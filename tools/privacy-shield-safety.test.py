# SPDX-License-Identifier: AGPL-3.0-or-later
"""Pure fixtures for embedded Shield helpers; never import Qt or open a camera."""
import ast
import hashlib
import io
import json
import os
from pathlib import Path
import re
import tempfile
import time
from types import SimpleNamespace
from collections import deque
import unittest
from unittest.mock import patch
import urllib.request

SOURCE = Path(__file__).resolve().parents[1] / "src-tauri/commander-free/scripts/modules/privacy/privacy_shield.ps1"
TEXT = SOURCE.read_text(encoding="utf-8")
PYTHON = TEXT.split("$embeddedScript = @'", 1)[1].split("\n'@", 1)[0]
TREE = ast.parse(PYTHON)
HELPERS = {"model_matches", "ModelNoRedirect", "ensure_verified_model", "purge_expired_captures", "write_ready_receipt"}
NODES = [node for node in TREE.body if isinstance(node, (ast.FunctionDef, ast.ClassDef)) and node.name in HELPERS]
assert len(NODES) == len(HELPERS)

class ShieldSafetyTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.folder = Path(self.temp.name)
        self.events = []
        self.logs = []
        self.env = {"os": os, "time": time, "re": re, "json": json,
                    "hashlib": hashlib, "tempfile": tempfile, "urllib": urllib,
                    "MAX_MODEL_BYTES": 1024, "MODEL_SHA256": {},
                    "log": self.logs.append, "emit_event": lambda *args: self.events.append(args),
                    "READY_FILE": str(self.folder / "ready.json"),
                    "DETECTOR_STARTED_AT": 100, "_last_ready_write": 0}
        exec(compile(ast.Module(body=NODES, type_ignores=[]), str(SOURCE), "exec"), self.env)
        self.body = b"fixture-model" * 10
        self.digest = hashlib.sha256(self.body).hexdigest()
        self.url = "https://storage.googleapis.com/mediapipe-models/fixture"
        self.env["MODEL_SHA256"][self.url] = self.digest
        self.path = self.folder / "model.task"

    def ensure(self, body):
        class Opener:
            def open(inner, url, timeout):
                return io.BytesIO(body)
        with patch.object(urllib.request, "build_opener", return_value=Opener()):
            return self.env["ensure_verified_model"](str(self.path), self.url)

    def test_large_forged_cache_is_rejected(self):
        self.path.write_bytes(b"x" * 50001)
        self.assertFalse(self.env["model_matches"](self.path, self.digest))

    def test_warm_verified_cache_needs_no_network(self):
        self.path.write_bytes(self.body)
        with patch.object(urllib.request, "build_opener", side_effect=AssertionError("network used")):
            self.assertTrue(self.env["ensure_verified_model"](str(self.path), self.url))

    def test_verified_download_is_published_atomically(self):
        self.path.write_bytes(b"old-cache")
        self.assertTrue(self.ensure(self.body))
        self.assertEqual(self.path.read_bytes(), self.body)
        self.assertEqual(list(self.folder.iterdir()), [self.path])

    def test_bad_download_keeps_previous_bytes_and_cleans_temporary(self):
        self.path.write_bytes(b"old-cache")
        self.assertFalse(self.ensure(b"forged"))
        self.assertEqual(self.path.read_bytes(), b"old-cache")
        self.assertEqual(list(self.folder.iterdir()), [self.path])

    def test_oversized_download_and_unknown_source_are_rejected(self):
        self.assertFalse(self.ensure(b"x" * 1025))
        self.assertFalse(self.path.exists())
        self.assertFalse(self.env["ensure_verified_model"](str(self.path), "https://example.invalid/model"))
        self.assertEqual(list(self.folder.iterdir()), [])

    def test_redirect_is_refused(self):
        with self.assertRaises(ValueError):
            self.env["ModelNoRedirect"]().redirect_request(None, None, 302, "", {}, "https://example.invalid")

    def media(self, name, age):
        path = self.folder / name
        path.write_bytes(b"synthetic media")
        os.utime(path, (1000000 - age, 1000000 - age))
        return path

    def test_retention_expires_only_owned_media_strictly_after_seven_days(self):
        expired = self.media("20260101_010101_phone_detected_screen.png", 7 * 86400 + 1)
        exact = self.media("20260101_010102_multiple_faces_webcam.avi", 7 * 86400)
        unrelated = self.media("user-note.png", 9 * 86400)
        fresh = self.media("20260101_010103_phone_detected_webcam.avi", 60)
        self.assertEqual(self.env["purge_expired_captures"](self.folder, 1000000), 0)
        self.assertFalse(expired.exists())
        for path in [exact, unrelated, fresh]:
            self.assertTrue(path.exists())

    def test_retention_failure_is_reported_and_retried(self):
        path = self.media("20260101_010101_multiple_faces_screen.png", 8 * 86400)
        with patch.object(os, "remove", side_effect=PermissionError("synthetic locked file")):
            self.assertEqual(self.env["purge_expired_captures"](self.folder, 1000000), 1)
        self.assertEqual(self.events, [("retention_cleanup_failed",)])
        self.assertEqual(self.env["purge_expired_captures"](self.folder, 1000000), 0)
        self.assertFalse(path.exists())

    def test_ready_receipt_updates_only_when_called_and_is_rate_limited(self):
        path = Path(self.env["READY_FILE"])
        self.assertFalse(path.exists())
        self.env["write_ready_receipt"](101)
        self.assertEqual(json.loads(path.read_text())["updatedAt"], 101)
        self.env["write_ready_receipt"](101.5)
        self.assertEqual(json.loads(path.read_text())["updatedAt"], 101)
        self.env["write_ready_receipt"](103)
        self.assertEqual(json.loads(path.read_text())["updatedAt"], 103)

    def run_worker(self, failure=None):
        worker_class = next(n for n in TREE.body if isinstance(n, ast.ClassDef) and n.name == "ShieldWorker")
        method = next(n for n in worker_class.body if isinstance(n, ast.FunctionDef) and n.name == "run")
        states, ready, failures = [], [], []
        signal = lambda sink: SimpleNamespace(emit=lambda *args: sink.append(args))
        worker = SimpleNamespace(
            _ensure_models=lambda: failure != "model", _init_detectors=lambda: True,
            init_failed=signal(failures), status_msg=signal([]), state_changed=signal(states),
            camera_idx=0, _running=True, _frame_buffer=deque(maxlen=30),
            check_phone=True, check_faces=False, check_gaze=False, confidence=0.5,
            capture_on_device=False, capture_on_multi_face=False,
            _captured_for_device=False, _captured_for_multi_face=False,
            _is_locked=True, _lock_reason="", _attentive_ms=0, _distracted_ms=0,
            wake_delay_ms=150, device_wake_multiplier=2, multi_face_wake_multiplier=2,
            buffer_frames=2, _active_recordings=[],
        )
        frame = SimpleNamespace(mean=lambda: 0 if failure == "black" else 10, copy=lambda: object())
        camera = SimpleNamespace(isOpened=lambda: failure != "camera", read=lambda: (True, frame), release=lambda: None)
        def detect(_):
            worker._running = False
            if failure == "inference":
                raise ValueError("synthetic inference failure")
            return SimpleNamespace(detections=[])
        worker.obj_detector = SimpleNamespace(detect=detect, close=lambda: None)
        worker.face_detector = None
        worker.mp = SimpleNamespace(Image=lambda **kwargs: object(), ImageFormat=SimpleNamespace(SRGB=1))
        class Event:
            def set(self): pass
            def wait(self, timeout): return True
        class Thread:
            def __init__(self, target, daemon): self.target = target
            def start(self): self.target()
        env = {"time": time, "threading": SimpleNamespace(Event=Event, Thread=Thread),
               "cv2": SimpleNamespace(VideoCapture=lambda *args: camera, CAP_DSHOW=1, COLOR_BGR2RGB=1, cvtColor=lambda *args: object()),
               "log": self.logs.append, "write_ready_receipt": lambda: ready.append(True)}
        exec(compile(ast.Module(body=[method], type_ignores=[]), str(SOURCE), "exec"), env)
        env["run"](worker)
        return states, ready, failures

    def test_actual_worker_emits_readiness_only_after_successful_inference(self):
        states, ready, failures = self.run_worker()
        self.assertEqual(ready, [True])
        self.assertEqual(failures, [])
        self.assertEqual(states, [(True, "")])

    def test_actual_worker_start_failures_never_emit_readiness(self):
        for failure in ["model", "camera", "black"]:
            with self.subTest(failure=failure):
                states, ready, failures = self.run_worker(failure)
                self.assertEqual(ready, [])
                self.assertEqual(states, [])
                self.assertEqual(len(failures), 1)

    def test_actual_worker_failed_inference_never_clears_existing_alert(self):
        states, ready, failures = self.run_worker("inference")
        self.assertEqual(states, [])
        self.assertEqual(ready, [])

    def test_runtime_start_has_no_hidden_package_install(self):
        before_python = TEXT.split("$embeddedScript = @'", 1)[0]
        self.assertNotIn("-m pip install", before_python)
        self.assertIn("Open AI runtime setup", before_python)

if __name__ == "__main__":
    unittest.main(verbosity=2)
