"""Exercise the detector's actual embedded, dependency-free gaze timer."""
import ast
from pathlib import Path
from types import SimpleNamespace
import tempfile
import unittest

SOURCE = Path(__file__).resolve().parents[1] / "src-tauri/commander-free/scripts/modules/privacy/privacy_shield.ps1"
script = SOURCE.read_text(encoding="utf-8").split("$embeddedScript = @'", 1)[1].split("\n'@", 1)[0]
tree = ast.parse(script)
timer = next(node for node in tree.body if isinstance(node, ast.ClassDef) and node.name == "ContinuousLookAway")
namespace = {}
exec(compile(ast.Module(body=[timer], type_ignores=[]), str(SOURCE), "exec"), namespace)
ContinuousLookAway = namespace["ContinuousLookAway"]
worker = next(node for node in tree.body if isinstance(node, ast.ClassDef) and node.name == "ShieldWorker")
run = next(node for node in worker.body if isinstance(node, ast.FunctionDef) and node.name == "run")
detect = next(node for node in ast.walk(run) if isinstance(node, ast.Try) and "raw_look_away" in ast.unparse(node))
detect_code = compile(ast.Module(body=[detect], type_ignores=[]), str(SOURCE), "exec")


class LookAwayTimerTests(unittest.TestCase):
    def test_fleet_capture_records_webcam_without_screen_grab(self):
        method = next(node for node in worker.body if isinstance(node, ast.FunctionDef) and node.name == "_capture_incident")
        captured = []
        local = {
            "os": SimpleNamespace(makedirs=lambda *_args, **_kwargs: None, path=SimpleNamespace(join=lambda *parts: "/".join(parts))),
            "time": SimpleNamespace(strftime=lambda *_args: "stamp"),
            "sys": SimpleNamespace(platform="win32"), "CAPTURES_DIR": "captures",
            "uuid": SimpleNamespace(uuid4=lambda: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"),
            "log": lambda value: captured.append(value),
        }
        exec(compile(ast.Module(body=[method], type_ignores=[]), str(SOURCE), "exec"), local)
        detector = SimpleNamespace(fleet_video_only=True, _capture_ids={}, _start_webcam_recording=lambda *args: captured.append(args) or True)
        local["_capture_incident"](detector, object(), "PHONE DETECTED", "device")
        self.assertEqual(captured[0], ("clip_aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", "device"))
        self.assertEqual(detector._capture_ids["device"], "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa")
        self.assertIn("webcam video", captured[1])

    def test_event_attaches_only_matching_capture_ids(self):
        app_class = next(node for node in tree.body if isinstance(node, ast.ClassDef) and node.name == "ShieldApp")
        method = next(node for node in app_class.body if isinstance(node, ast.FunctionDef) and node.name == "_emit_look_event")
        events = []
        local = {"emit_event": lambda *args: events.append(args)}
        exec(compile(ast.Module(body=[method], type_ignores=[]), str(SOURCE), "exec"), local)
        app = SimpleNamespace(args=SimpleNamespace(fleet_video_only=True))
        ids = {"device": "phone-id", "multi_face": "face-id"}
        local["_emit_look_event"](app, False, "PHONE DETECTED", ids)
        self.assertEqual(events[-1][2], {"phone_detected": "phone-id"})
        local["_emit_look_event"](app, True, "", ids)
        self.assertEqual(events[-1][2], {})

    def test_fleet_recording_has_a_thirty_second_limit(self):
        method = next(node for node in worker.body if isinstance(node, ast.FunctionDef) and node.name == "_start_webcam_recording")
        output = SimpleNamespace(isOpened=lambda: True, write=lambda _frame: None)
        local = {
            "os": SimpleNamespace(path=SimpleNamespace(join=lambda *parts: "/".join(parts))),
            "CAPTURES_DIR": "captures", "time": SimpleNamespace(monotonic=lambda: 100.0),
            "cv2": SimpleNamespace(VideoWriter_fourcc=lambda *_args: 0, VideoWriter=lambda *_args: output),
            "log": lambda _value: None,
        }
        exec(compile(ast.Module(body=[method], type_ignores=[]), str(SOURCE), "exec"), local)
        frame = SimpleNamespace(shape=(480, 640, 3))
        detector = SimpleNamespace(_frame_buffer=[frame], _active_recordings=[], capture_speed=1, fleet_video_only=True)
        self.assertTrue(local["_start_webcam_recording"](detector, "clip_id", "device"))
        self.assertEqual(detector._active_recordings[0]["max_until"], 130.0)
        self.assertEqual(detector._active_recordings[0]["path"], "captures/clip_id_pending.avi")
        self.assertEqual(detector._active_recordings[0]["final_path"], "captures/clip_id_webcam.avi")

    def test_fleet_video_is_published_only_after_writer_is_closed(self):
        import os
        method = next(node for node in worker.body if isinstance(node, ast.FunctionDef) and node.name == "_finish_webcam_recording")
        local = {"os": os, "log": lambda _value: None}
        exec(compile(ast.Module(body=[method], type_ignores=[]), str(SOURCE), "exec"), local)
        with tempfile.TemporaryDirectory() as directory:
            pending, complete = Path(directory) / "clip_pending.avi", Path(directory) / "clip_webcam.avi"
            pending.write_bytes(b"video")
            released = []
            recording = {"out": SimpleNamespace(release=lambda: released.append(complete.exists())),
                         "path": str(pending), "final_path": str(complete)}
            local["_finish_webcam_recording"](None, recording)
            self.assertEqual(released, [False])
            self.assertFalse(pending.exists())
            self.assertEqual(complete.read_bytes(), b"video")
            pending.write_bytes(b"")
            complete.unlink()
            local["_finish_webcam_recording"](None, recording)
            self.assertTrue(pending.exists())
            self.assertFalse(complete.exists())
            pending.write_bytes(b"partial")
            recording["out"] = SimpleNamespace(release=lambda: (_ for _ in ()).throw(OSError("release failed")))
            local["_finish_webcam_recording"](None, recording)
            self.assertTrue(pending.exists())
            self.assertFalse(complete.exists())

    def test_failed_writer_is_closed_without_publishing_a_clip(self):
        method = next(node for node in worker.body if isinstance(node, ast.FunctionDef) and node.name == "_start_webcam_recording")
        released = []
        output = SimpleNamespace(isOpened=lambda: False, release=lambda: released.append(True))
        local = {
            "os": SimpleNamespace(path=SimpleNamespace(join=lambda *parts: "/".join(parts))),
            "CAPTURES_DIR": "captures", "time": SimpleNamespace(monotonic=lambda: 100.0),
            "cv2": SimpleNamespace(VideoWriter_fourcc=lambda *_args: 0, VideoWriter=lambda *_args: output),
            "log": lambda _value: None,
        }
        exec(compile(ast.Module(body=[method], type_ignores=[]), str(SOURCE), "exec"), local)
        detector = SimpleNamespace(_frame_buffer=[SimpleNamespace(shape=(480, 640, 3))],
                                   _active_recordings=[], capture_speed=1, fleet_video_only=True)
        self.assertFalse(local["_start_webcam_recording"](detector, "clip_id", "device"))
        self.assertEqual(released, [True])
        self.assertEqual(detector._active_recordings, [])

    def test_oversized_completed_recording_is_not_published(self):
        method = next(node for node in worker.body if isinstance(node, ast.FunctionDef) and node.name == "_finish_webcam_recording")
        renamed = []
        local = {"os": SimpleNamespace(path=SimpleNamespace(getsize=lambda _path: 50 * 1024 * 1024 + 1),
                                       rename=lambda *args: renamed.append(args)),
                 "log": lambda _value: None}
        exec(compile(ast.Module(body=[method], type_ignores=[]), str(SOURCE), "exec"), local)
        recording = {"out": SimpleNamespace(release=lambda: None), "path": "pending.avi", "final_path": "ready.avi"}
        local["_finish_webcam_recording"](None, recording)
        self.assertEqual(renamed, [])
    def test_default_gaze_and_explicit_all_off_are_distinct(self):
        app_class = next(node for node in tree.body if isinstance(node, ast.ClassDef) and node.name == "ShieldApp")
        init = next(node for node in app_class.body if isinstance(node, ast.FunctionDef) and node.name == "__init__")
        default = next(node for node in init.body if isinstance(node, ast.If) and "detector_options_explicit" in ast.unparse(node.test))
        code = compile(ast.Module(body=[default], type_ignores=[]), str(SOURCE), "exec")
        for explicit, expected in [(False, True), (True, False)]:
            args = SimpleNamespace(detector_options_explicit=explicit, check_gaze=False, check_faces=False, check_phone=False)
            exec(code, {"self": SimpleNamespace(args=args)})
            self.assertEqual(args.check_gaze, expected)

    def test_immediate_local_default_and_ten_second_managed_delay(self):
        self.assertTrue(ContinuousLookAway(0).observe(True, 50.0))
        detector = ContinuousLookAway(10)
        self.assertFalse(detector.observe(True, 50.0))
        for second in range(51, 60):
            self.assertFalse(detector.observe(True, float(second)))
        self.assertTrue(detector.observe(True, 60.0))

    def test_attentive_and_lost_reset_the_continuous_episode(self):
        detector = ContinuousLookAway(3)
        self.assertFalse(detector.observe(True, 1.0))
        self.assertFalse(detector.observe(True, 2.0))
        self.assertFalse(detector.observe(False, 2.5))
        self.assertFalse(detector.observe(True, 3.0))
        self.assertFalse(detector.observe(True, 4.0))
        detector.reset()  # missing frame, unknown face, or detector failure
        self.assertFalse(detector.observe(True, 5.0))
        self.assertFalse(detector.observe(True, 6.0))
        self.assertFalse(detector.observe(True, 7.0))
        self.assertTrue(detector.observe(True, 8.0))

    def test_unobserved_gap_or_backwards_clock_cannot_count_as_away(self):
        detector = ContinuousLookAway(3)
        self.assertFalse(detector.observe(True, 1.0))
        self.assertFalse(detector.observe(True, 20.0))
        self.assertFalse(detector.observe(True, 19.0))
        self.assertFalse(detector.observe(True, 20.0))
        self.assertFalse(detector.observe(True, 21.0))
        self.assertTrue(detector.observe(True, 22.0))

    def test_real_detection_block_delays_gaze_without_delaying_phone_or_presence(self):
        detector = SimpleNamespace(check_phone=True, check_faces=True, check_gaze=True, buffer_frames=1,
            _device_detected_streak=0, _multi_face_detected_streak=0, _no_face_detected_streak=0,
            _face_recovery_streak=0, _presence_loss_active=False, _look_away=ContinuousLookAway(3),
            phone=False, face=True, away=True)
        detector.obj_detector = SimpleNamespace(detect=lambda _: SimpleNamespace(detections=[SimpleNamespace(categories=[SimpleNamespace(category_name="cell phone")])] if detector.phone else []))
        detector.face_detector = SimpleNamespace(detect=lambda _: SimpleNamespace(face_landmarks=[object()] if detector.face else []))
        detector.profile = False
        detector._face_orientation = lambda _: "profile" if detector.profile else "away" if detector.away else "attentive"
        def frame(now):
            env = {"self": detector, "mp_image": object(), "is_clear": True, "reason": "", "time": SimpleNamespace(monotonic=lambda: now), "log": lambda _: None}
            exec(detect_code, env)
            return env["reason"]
        self.assertEqual(frame(1.0), "")
        detector.phone = True
        self.assertEqual(frame(2.0), "PHONE DETECTED")
        detector.phone = False
        self.assertEqual(frame(3.0), "")
        detector.profile = True
        self.assertEqual(frame(3.5), "PRESENCE LOST")  # independent of look-away delay
        detector.profile = False
        self.assertEqual(frame(4.0), "")  # profile resets the mild-away timer
        self.assertEqual(frame(5.0), "")
        self.assertEqual(frame(6.0), "")
        self.assertEqual(frame(7.0), "LOOK AWAY")
        detector.face = False
        self.assertEqual(frame(8.0), "PRESENCE LOST")
        detector.face = True
        self.assertEqual(frame(9.0), "")

    def test_real_orientation_classifies_slight_and_full_profile_without_extra_inference(self):
        orientation = next(node for node in worker.body if isinstance(node, ast.FunctionDef) and node.name == "_face_orientation")
        local = {}
        exec(compile(ast.Module(body=[orientation], type_ignores=[]), str(SOURCE), "exec"), local)
        def classify(yaw):
            points = [SimpleNamespace(x=0.0) for _ in range(455)]
            points[234].x, points[454].x, points[1].x = 0, 1, 0.5 + yaw
            return local["_face_orientation"](None, points)
        self.assertEqual(classify(0.2), "attentive")
        self.assertEqual(classify(0.4), "away")
        self.assertEqual(classify(-0.48), "profile")

    def test_overlay_uses_blur_for_mild_away_and_black_with_label_for_presence(self):
        overlay_class = next(node for node in tree.body if isinstance(node, ast.ClassDef) and node.name == "ShieldOverlay")
        method = next(node for node in overlay_class.body if isinstance(node, ast.FunctionDef) and node.name == "update_state")
        local = {}
        exec(compile(ast.Module(body=[method], type_ignores=[]), str(SOURCE), "exec"), local)
        events = []
        overlay = SimpleNamespace(opacity_val=200, hide=lambda: events.append("hide"),
            show=lambda: events.append("show"), enable_blur=lambda: None,
            setStyleSheet=lambda value: events.append(value),
            label=SimpleNamespace(hide=lambda: events.append("hide label"),
                show=lambda: events.append("show label"), setText=lambda text: events.append(text)))
        update = local["update_state"]
        update(overlay, False, "LOOK AWAY")
        self.assertEqual(overlay._active_opacity, 120)
        self.assertIn("hide label", events)
        events.clear()
        update(overlay, False, "PRESENCE LOST")
        self.assertEqual(overlay._active_opacity, 255)
        self.assertIn("QWidget#shieldOverlay { background-color: black; }", events)
        self.assertIn("PRESENCE LOST", events)
        self.assertIn("show label", events)

    def test_wake_requires_continuous_attentive_time_and_unknown_resets_it(self):
        # Execute the real smoothing and transition blocks with fake signals.
        loop = next(node for node in run.body if isinstance(node, ast.While))
        blocks = [node for node in loop.body if isinstance(node, ast.If)]
        state_blocks = [node for node in blocks if ast.unparse(node.test) == "recovery_unknown and is_clear"]
        smoothing, transition = state_blocks
        recovery = next(node for node in loop.body if isinstance(node, ast.Assign) and any(isinstance(target, ast.Name) and target.id == "recovery_unknown" for target in node.targets))
        code = compile(ast.Module(body=[recovery, smoothing, transition], type_ignores=[]), str(SOURCE), "exec")
        events = []
        detector = SimpleNamespace(_attentive_ms=0, _distracted_ms=0, _is_locked=True,
            _lock_reason="PRESENCE LOST", wake_delay_ms=300, device_wake_multiplier=2,
            multi_face_wake_multiplier=2, buffer_frames=2, _captured_for_device=False,
            _captured_for_multi_face=False, _capture_ids={}, state_changed=SimpleNamespace(emit=lambda *x: events.append(x)))
        def frame(clear=True, unknown=False, away=False, phone=False, reason="", elapsed=100):
            env = dict(self=detector, is_clear=clear, presence_unknown=unknown, raw_look_away=away, delta_ms=elapsed,
                reason=reason, device_triggered=phone, multi_face_triggered=False,
                gaze_triggered=not clear, presence_lost_triggered=False)
            exec(code, env)
        frame(); frame(); self.assertEqual(events, [])
        frame(unknown=True); frame(); frame(); self.assertEqual(events, [])
        frame(clear=False, reason="LOOK AWAY"); frame(); frame(); self.assertEqual(events, [(False, "LOOK AWAY", {})])
        frame(); self.assertEqual(events[-1], (True, "", {}))

    def test_pending_mild_away_cannot_clear_lock_and_phone_survives_unknown_face(self):
        loop = next(node for node in run.body if isinstance(node, ast.While))
        blocks = [node for node in loop.body if isinstance(node, ast.If) and ast.unparse(node.test) == "recovery_unknown and is_clear"]
        recovery = next(node for node in loop.body if isinstance(node, ast.Assign) and any(isinstance(target, ast.Name) and target.id == "recovery_unknown" for target in node.targets))
        code = compile(ast.Module(body=[recovery, *blocks], type_ignores=[]), str(SOURCE), "exec")
        events = []
        detector = SimpleNamespace(_attentive_ms=0, _distracted_ms=0, _is_locked=True,
            _lock_reason="PRESENCE LOST", wake_delay_ms=300, device_wake_multiplier=2,
            multi_face_wake_multiplier=2, buffer_frames=2, _captured_for_device=False,
            _captured_for_multi_face=False, _capture_ids={}, state_changed=SimpleNamespace(emit=lambda *x: events.append(x)))
        def frame(clear, unknown=False, away=False, reason="", elapsed=100):
            exec(code, dict(self=detector, is_clear=clear, presence_unknown=unknown, raw_look_away=away,
                delta_ms=elapsed, reason=reason))
        frame(True); frame(True, away=True); frame(True); frame(True)
        self.assertEqual(events, [])
        frame(True); self.assertEqual(events, [(True, "", {})])
        frame(False, unknown=True, reason="PHONE DETECTED")
        self.assertEqual(events[-1], (False, "PHONE DETECTED", {}))


if __name__ == "__main__":
    unittest.main()
