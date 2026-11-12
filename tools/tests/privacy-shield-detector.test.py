"""Exercise the shipped inline detector with synthetic pixels; never open a camera."""
import ast
import ctypes
import json
import os
from pathlib import Path
import sys
import tempfile
import time
from types import SimpleNamespace
import unittest
from unittest.mock import patch
import uuid
from collections import deque

os.environ["QT_QPA_PLATFORM"] = "offscreen"
import cv2
import numpy as np
from PIL import Image
from PyQt6.QtCore import Qt, QTimer, pyqtSignal, QObject, QThread, QRect
from PyQt6.QtGui import QColor, QFont, QPainter, QImage, QPixmap
from PyQt6.QtWidgets import QApplication, QWidget, QLabel, QVBoxLayout

SOURCE = Path(__file__).resolve().parents[2] / "src-tauri/commander-free/scripts/modules/privacy/privacy_shield.ps1"
inline = SOURCE.read_text(encoding="utf-8-sig").split("$embeddedScript = @'", 1)[1].split("\n'@", 1)[0]
tree = ast.parse(inline)
selected = [node for node in tree.body if isinstance(node, (ast.FunctionDef, ast.ClassDef))]
env = dict(globals(), log=lambda _: None, emit_event=lambda *_: None)
exec(compile(ast.Module(body=selected, type_ignores=[]), str(SOURCE), "exec"), env)
app = QApplication.instance() or QApplication([])


def landmarks(yaw=0.0, left=0.25, right=0.75):
    points = [SimpleNamespace(x=0.5) for _ in range(455)]
    points[234].x, points[454].x = left, right
    points[1].x = (left + right) / 2 + yaw * (right - left)
    return points


class GazeTests(unittest.TestCase):
    def test_missing_landmark_frame_keeps_gaze_protection_until_presence_loss(self):
        worker = env["ShieldWorker"](buffer_frames=2)
        worker._lock_reason = "LOOK AWAY"
        for _ in range(5):
            self.assertEqual(worker._observe_missing_face(), (False, True))
        self.assertEqual(worker._observe_missing_face(), (True, False))

    def test_symmetric_mild_turn_blurs_strong_turn_blacks_out(self):
        classify = env["classify_gaze"]
        for sign in [-1, 1]:
            self.assertEqual(classify(landmarks(sign * 0.04)), "clear")
            self.assertEqual(classify(landmarks(sign * 0.2)), "blur")
            self.assertEqual(classify(landmarks(sign * 0.4)), "blur")
            self.assertEqual(classify(landmarks(sign * 0.5)), "blackout")
        self.assertEqual(classify(landmarks(left=0.01)), "blur")
        self.assertEqual(classify(landmarks(yaw=0.5, left=-0.1)), "blur")
        half_face = landmarks()
        half_face[10].y = -0.1
        self.assertEqual(classify(half_face), "blur")
        self.assertEqual(classify(landmarks(left=0.5, right=0.5)), "blackout")
        self.assertEqual(classify(landmarks(float("nan"))), "blackout")

    def test_debug_reason_distinguishes_turn_from_frame_edge_without_values(self):
        classify = env["classify_gaze_with_reason"]
        self.assertEqual(classify(landmarks()), ("clear", "clear"))
        self.assertEqual(classify(landmarks(0.2)), ("blur", "yaw"))
        self.assertEqual(classify(landmarks(left=0.01)), ("blur", "frame_edge"))
        self.assertEqual(classify(landmarks(yaw=0.5, left=-0.1)), ("blur", "frame_edge"))
        self.assertEqual(classify(landmarks(0.5)), ("blackout", "full_turn"))

    def test_short_noise_does_not_blur_and_full_turn_can_escalate(self):
        state = env["GazeState"](2)
        self.assertEqual(state.observe("blur"), "clear")
        self.assertEqual(state.observe("clear"), "clear")
        self.assertEqual(state.observe("blur"), "clear")
        self.assertEqual(state.observe("blur"), "blur")
        self.assertEqual(state.observe("blackout"), "blur")
        self.assertEqual(state.observe("blackout"), "blur")
        self.assertEqual(state.observe("blackout"), "blackout")
        self.assertEqual(state.observe("clear"), "blackout")
        self.assertEqual(state.observe("clear"), "clear")

    def test_full_turn_has_message_and_mild_turn_reuses_blurred_background(self):
        image = QImage(128, 128, QImage.Format.Format_RGBA8888)
        image.fill(QColor("white"))
        painter = QPainter(image)
        painter.fillRect(64, 0, 64, 128, QColor("blue"))
        painter.end()
        screen = SimpleNamespace(geometry=lambda: QRect(0, 0, 128, 128), grabWindow=lambda _: QPixmap.fromImage(image))
        overlay = env["ShieldOverlay"](screen)
        overlay.prepare_protection()
        self.assertIsNotNone(overlay.blurred_screen)
        blurred = overlay.blurred_screen.toImage()
        pixel = blurred.pixelColor(blurred.width() // 2 - 1, blurred.height() // 2)
        self.assertGreater(pixel.blue(), 0)
        self.assertLess(pixel.red(), 250)
        key = overlay.blurred_screen.cacheKey()
        overlay.update_state(False, "LOOK AWAY")
        self.assertEqual(overlay.presentation, "blur")
        self.assertTrue(overlay.label.isHidden())
        overlay.update_state(False, "LOOK AWAY (FULL TURN)")
        self.assertEqual(overlay.presentation, "blackout")
        self.assertFalse(overlay.label.isHidden())
        self.assertEqual(overlay.label.text(), "LOOK AWAY")
        self.assertNotIn("FULL TURN", overlay.label.text())
        self.assertIn("rgba(255, 50, 50, 200)", overlay.label.styleSheet())
        self.assertIn("Consolas", overlay.label.styleSheet())
        self.assertEqual(overlay.grab().toImage().pixelColor(0, 0), QColor("black"))
        overlay.update_state(False, "PRESENCE LOST")
        self.assertEqual(overlay.presentation, "blackout")
        self.assertEqual(overlay.label.text(), "PRESENCE LOST")
        self.assertEqual(overlay.grab().toImage().pixelColor(0, 0), QColor("black"))
        overlay.update_state(False, "LOOK AWAY")
        self.assertEqual(overlay.blurred_screen.cacheKey(), key)
        overlay.update_state(True, "")
        self.assertIsNone(overlay.blurred_screen)
        self.assertFalse(overlay.isVisible())


class CaptureTests(unittest.TestCase):
    def setUp(self):
        self.folder = tempfile.TemporaryDirectory(prefix="wc-shield-synthetic-")
        env["CAPTURES_DIR"] = self.folder.name
        self.worker = env["ShieldWorker"]()
        self.messages = []
        self.worker.status_msg.connect(self.messages.append)
        self.frame = np.full((48, 64, 3), 120, dtype=np.uint8)
        for i in range(6):
            self.worker._frame_buffer.append(self.frame.copy())
            self.worker._frame_times.append(i / 10)

    def tearDown(self):
        for rec in self.worker._active_recordings:
            rec["out"].release()
        self.folder.cleanup()

    def capture(self):
        with patch("PIL.ImageGrab.grab", return_value=Image.new("RGB", (64, 48), "blue")) as grab:
            result = self.worker._capture_incident(self.frame, "PHONE DETECTED", "device")
            grab.assert_called_once_with(all_screens=True)
            return result

    def test_real_synthetic_png_and_avi_are_saved_and_decodable(self):
        self.assertTrue(self.capture())
        manifest = next(Path(self.folder.name).glob("*_capture.json"))
        self.assertEqual(json.loads(manifest.read_text())["status"], "recording")
        with Image.open(next(Path(self.folder.name).glob("*.png"))) as screenshot:
            self.assertEqual(screenshot.size, (64, 48))
        self.worker._update_recordings(self.frame, False, False, 100)
        self.worker._update_recordings(self.frame, False, False, 106)
        result = json.loads(manifest.read_text())
        self.assertEqual(result["status"], "complete")
        self.assertTrue(result["webcamSaved"])
        video = cv2.VideoCapture(str(next(Path(self.folder.name).glob("*.avi"))))
        try:
            self.assertTrue(video.read()[0])
            self.assertAlmostEqual(video.get(cv2.CAP_PROP_FPS), 10, delta=0.1)
        finally:
            video.release()

    def test_failed_screenshot_does_not_prevent_video(self):
        with patch("PIL.ImageGrab.grab", side_effect=OSError("synthetic failure")):
            self.assertTrue(self.worker._capture_incident(self.frame, "PHONE DETECTED", "device"))
        self.assertEqual(len(self.worker._active_recordings), 1)
        self.worker._update_recordings(self.frame, False, False, 100)
        self.worker._update_recordings(self.frame, False, False, 106)
        result = json.loads(next(Path(self.folder.name).glob("*_capture.json")).read_text())
        self.assertEqual(result["status"], "partial")
        self.assertFalse(result["screenSaved"])
        self.assertTrue(result["webcamSaved"])

    def test_writer_failure_is_partial_and_unique_incidents_do_not_overwrite(self):
        with patch.object(cv2, "VideoWriter") as factory:
            factory.return_value.isOpened.return_value = False
            self.assertTrue(self.capture())
            self.assertTrue(self.capture())
            self.assertEqual(factory.return_value.release.call_count, 2)
        manifests = list(Path(self.folder.name).glob("*_capture.json"))
        self.assertEqual(len(manifests), 2)
        self.assertTrue(all(json.loads(p.read_text())["status"] == "partial" for p in manifests))

    def test_retries_are_bounded_and_each_reason_rearms_independently(self):
        with patch.object(self.worker, "_capture_incident", side_effect=[False, True, True]) as capture:
            for now in [0, 1, 4]:
                self.worker._capture_if_needed(self.frame, True, True, "device", "PHONE DETECTED", now)
            self.assertEqual(capture.call_count, 1)
            self.worker._capture_if_needed(self.frame, True, True, "device", "PHONE DETECTED", 5)
            self.worker._capture_if_needed(self.frame, True, True, "device", "PHONE DETECTED", 6)
            self.assertEqual(capture.call_count, 2)
            self.worker._capture_if_needed(self.frame, False, True, "device", "PHONE DETECTED", 7)
            self.worker._captured_for_multi_face = True
            self.worker._capture_if_needed(self.frame, True, True, "device", "PHONE DETECTED", 8)
            self.assertEqual(capture.call_count, 3)

    def test_resumed_incident_extends_postroll_and_explicit_stop_flushes(self):
        self.assertTrue(self.capture())
        self.worker._update_recordings(self.frame, False, False, 100)
        self.assertTrue(self.worker._active_recordings[0]["in_post_roll"])
        self.worker._update_recordings(self.frame, True, False, 104)
        self.assertFalse(self.worker._active_recordings[0]["in_post_roll"])
        self.assertEqual(self.worker._active_recordings[0]["record_until"], 109)
        rec = self.worker._active_recordings.pop()
        self.worker._finish_recording(rec)
        self.assertEqual(json.loads(Path(rec["metadata_path"]).read_text())["status"], "complete")

    def test_disabled_capture_never_saves(self):
        with patch.object(self.worker, "_capture_incident") as capture:
            self.worker._capture_if_needed(self.frame, True, False, "device", "PHONE DETECTED", 0)
            capture.assert_not_called()

    def test_stop_request_targets_only_this_detector_and_rejects_bad_input(self):
        env["STOP_FILE"] = str(Path(self.folder.name) / "stop.json")
        controller = env["ShieldApp"].__new__(env["ShieldApp"])
        QObject.__init__(controller)
        controller.worker = self.worker
        controller.overlay = SimpleNamespace(update_state=lambda *_: None)
        stopped = []
        controller._stop_watchdog = SimpleNamespace(stop=lambda: stopped.append(True))
        for content in ['broken json', json.dumps({"pid": os.getpid() + 1}), ' ' * 129]:
            Path(env["STOP_FILE"]).write_text(content)
            controller._stop_when_requested()
            self.assertTrue(self.worker._running)
        Path(env["STOP_FILE"]).write_text(json.dumps({"pid": os.getpid()}))
        controller._stop_when_requested()
        self.assertFalse(self.worker._running)
        self.assertEqual(stopped, [True])


if __name__ == "__main__":
    unittest.main()
