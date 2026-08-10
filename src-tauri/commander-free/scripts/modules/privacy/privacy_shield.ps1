# ============================================================================
# PRIVACY SHIELD - LAUNCHER
# Python-based privacy shield
# ============================================================================


function Resolve-PythonPath {
    # mediapipe requires Python <= 3.12. This function only returns a 3.12.x interpreter.
    # Refresh PATH to catch new installations
    $env:Path = [System.Environment]::GetEnvironmentVariable("Path", "Machine") + ";" + [System.Environment]::GetEnvironmentVariable("Path", "User")
    
    # Helper: returns $true only if exe reports Python 3.12.x
    function Test-IsPy312 { param([string]$Exe)
        if (-not (Test-Path $Exe -ErrorAction SilentlyContinue)) { return $false }
        if ($Exe -match 'WindowsApps') { return $false }
        try { $v = & $Exe --version 2>&1 | Out-String; return ($v -match 'Python 3\.12\.') } catch { return $false }
    }

    # 1. Well-known Python 3.12 directories (highest priority)
    $candidates = @(
        "$env:LOCALAPPDATA\Programs\Python\Python312\python.exe",
        "$env:ProgramFiles\Python312\python.exe",
        "$env:ProgramFiles(x86)\Python312\python.exe",
        "C:\Python312\python.exe"
    )
    foreach ($c in $candidates) { if (Test-IsPy312 $c) { return $c } }

    # 2. Registry (HKLM then HKCU, 3.12 only)
    foreach ($root in @("HKLM:\SOFTWARE\Python\PythonCore", "HKCU:\SOFTWARE\Python\PythonCore")) {
        $regPath = "$root\3.12\InstallPath"
        if (Test-Path $regPath) {
            $installDir = (Get-ItemProperty -Path $regPath -ErrorAction SilentlyContinue).'(default)'
            if ($installDir) {
                $exe = Join-Path $installDir 'python.exe'
                if (Test-IsPy312 $exe) { return $exe }
            }
        }
    }

    # 3. Walk PATH - skip anything that isn't 3.12
    foreach ($dir in ($env:Path -split ';')) {
        if ($dir -match 'WindowsApps') { continue }
        $exe = Join-Path $dir 'python.exe'
        if (Test-IsPy312 $exe) { return $exe }
    }

    return $null
}



function Install-AIDependencies {
    param([string]$Target = $null)
    # Delegate to centralized dependencies module
    if (Get-Command "Install-PrivacyShieldAI" -ErrorAction SilentlyContinue) {
        return Install-PrivacyShieldAI -Target $Target
    }
    return @{ error = $true; message = "Dependencies module not loaded." }
}

function Get-PrivacyShieldCameraAvailability {
    try {
        $devices = @()

        foreach ($className in @("Camera", "Image")) {
            try {
                $devices += Get-CimInstance -ClassName Win32_PnPEntity `
                    -Filter "PNPClass='$className'" `
                    -ErrorAction SilentlyContinue |
                    Where-Object {
                        $_.Status -eq "OK" -and
                        $_.Name -and
                        $_.Name -notmatch '(?i)\bvirtual\b|obs|snap\s*camera'
                    } |
                    ForEach-Object { $_.Name }
            } catch {}
        }

        $names = @($devices | Sort-Object -Unique)
        @{
            available = ($names.Count -gt 0)
            devices   = $names
            message   = if ($names.Count -gt 0) { "Camera available." } else { "No usable camera detected." }
        }
    }
    catch {
        @{ available = $false; devices = @(); message = $_.Exception.Message }
    }
}

function Get-PrivacyShieldStatus {
    try {
        $shieldProcessMarker = "--wc-privacy-shield"
        $running = $false
        $processId = $null
        $camera = Get-PrivacyShieldCameraAvailability

        # Get-CimInstance works reliably on both PowerShell 5.1 (Windows
        # default) AND PowerShell 7+. The previous Get-WmiObject path
        # silently returns nothing under PS 7 because Get-WmiObject was
        # removed -- that broke the status read entirely and the button
        # stayed stuck on "Activate Shield" while Python kept running.
        # Filter at the CIM level (-Filter) instead of pipe-Where so we
        # don't materialise the whole process list just to drop most of
        # it.
        $procs = Get-CimInstance -ClassName Win32_Process `
            -Filter "Name='python.exe' OR Name='pythonw.exe'" `
            -ErrorAction SilentlyContinue
        foreach ($proc in $procs) {
            if ($proc.CommandLine -like "*$shieldProcessMarker*") {
                $running = $true
                $processId = $proc.ProcessId
                break
            }
        }

        # NOTE: the old "last-ditch fallback" that grabbed the first
        # python.exe via Get-Process was removed. It didn't check the
        # --wc-privacy-shield marker and therefore produced false positives
        # on any machine that had an unrelated Python process (e.g. Windows
        # Server 2025 with a monitoring agent). CIM works on both PS 5.1 and
        # PS 7, so the fallback is not needed.

        @{
            running         = $running
            processId       = $processId
            cameraAvailable = [bool]$camera.available
            cameraDevices   = @($camera.devices)
            cameraMessage   = $camera.message
        }
    }
    catch {
        @{ error = $true; message = $_.Exception.Message }
    }
}

function Start-PrivacyShield {
    param(
        [int]$Camera = 0,
        [switch]$CheckGaze,
        [switch]$CheckFaces,
        [switch]$CheckPhone,
        # Fleet can monitor every detector while these independent controls
        # decide which detected conditions blur the endpoint displays.
        [switch]$BlurGaze,
        [switch]$BlurFaces,
        [switch]$BlurPhone,
        [switch]$CaptureOnDevice,
        [switch]$CaptureOnMultiFace,
        [string]$ModelLevel = "nano",
        [float]$Confidence = 0.5,
        [int]$OverlayOpacity = 200,
        [int]$WakeDelayMs = 150,
        [int]$DeviceWakeMultiplier = 2,
        [int]$MultiFaceWakeMultiplier = 2,
        [int]$BufferFrames = 2,
        [int]$CaptureSpeed = 1
    )
    
    try {
        $status = Get-PrivacyShieldStatus
        if ($status.running) {
            return @{ error = $true; message = "Shield is already running." }
        }
        if ($status.cameraAvailable -ne $true) {
            return @{
                error = $true
                message = "No camera detected - Privacy Shield requires a webcam."
                cameraAvailable = $false
                cameraDevices = @()
            }
        }

        $pythonExe = Resolve-PythonPath
        if (-not $pythonExe) {
            return @{ error = $true; message = "Python is required." }
        }

        # Check dependencies (Quietly)
        # Check dependencies (Quietly)
        $packages = @("mediapipe", "opencv-python", "PyQt6", "numpy", "Pillow")
        $importMap = @{
            "opencv-python" = "cv2"
            "Pillow"        = "PIL"
        }
        foreach ($pkg in $packages) {
            $importName = if ($importMap.ContainsKey($pkg)) { $importMap[$pkg] } else { $pkg.Split('-')[0] }
            try {
                # Just invoke python, silencing output
                & $pythonExe -c "import $importName" *>$null
                if ($LASTEXITCODE -ne 0) {
                    # Pip install can be chatty, silence it completely
                    & $pythonExe -m pip install $pkg --quiet *>$null
                }
                & $pythonExe -c "import $importName" *>$null
                if ($LASTEXITCODE -ne 0) {
                    return @{ error = $true; message = "Missing Python dependency: $pkg" }
                }
            }
            catch {}
        }

        # Keep the core script in-memory to avoid writing source code to AppData.
        $shieldProcessMarker = "--wc-privacy-shield"
        $embeddedScript = @'
"""
Unified Privacy Shield Module
=============================
Combines:
1. Gaze Detection (Blur on look away)
2. Multi-Face Detection (Blur on >1 face)
3. Object Detection (Blur on Phone/Camera)

Optimized for low-overhead running on CPU efficiently.
"""

import sys
import os
import json
import time
import argparse
import threading
import traceback
import ctypes
import urllib.request
from collections import deque
from ctypes import wintypes
from dataclasses import dataclass

# --- Basic Logging ---
LOG_FILE = os.path.join(os.environ.get("LOCALAPPDATA", ""), "WinCommander", "logs", "privacy_shield.log")
CAPTURES_DIR = os.path.join(os.environ.get("LOCALAPPDATA", ""), "WinCommander", "shield_captures")
# Durable NDJSON sidecar of look-state transitions. The PowerShell wrapper
# exits ~8s after spawning us, so stdout is not a reliable channel back to
# WinCommander; this file survives the wrapper and is tailed live by the
# Rust backend (backend.rs shield event reader).
EVENTS_FILE = os.path.join(os.environ.get("LOCALAPPDATA", ""), "WinCommander", "logs", "privacy_shield_events.ndjson")
os.makedirs(os.path.dirname(LOG_FILE), exist_ok=True)

def log(msg):
    try:
        timestamp = time.strftime("%Y-%m-%d %H:%M:%S")
        with open(LOG_FILE, "a", encoding="utf-8") as handle:
            handle.write(f"[{timestamp}] {msg}\n")
    except Exception:
        pass

def emit_event(name, reason=""):
    # One JSON object per line (NDJSON). Appended + flushed on every Qt
    # look-state change so the backend reader sees transitions promptly.
    try:
        line = json.dumps({"event": name, "reason": reason, "ts": time.time()})
        with open(EVENTS_FILE, "a", encoding="utf-8") as handle:
            handle.write(line + "\n")
            handle.flush()
    except Exception:
        pass


# --- Parent-PID watchdog ---
# Backstop for the Rust-side kill-on-close Job Object. If WinCommander
# is end-tasked or BSODs, this thread notices the parent process is
# gone and self-terminates. Without it the Python shield can outlive
# its owner indefinitely (the PowerShell wrapper that spawned us has
# already exited, so our OS parent is services.exe -- the Job Object
# is the only thing holding us; if the OS denies job assignment
# (nested-job restriction) we never get killed).
def _parent_watchdog(parent_pid):
    if not parent_pid or parent_pid <= 0:
        return
    # Open the parent with SYNCHRONIZE so we can WaitForSingleObject
    # on it. When the parent dies, the wait returns and we exit.
    PROCESS_SYNCHRONIZE = 0x00100000
    INFINITE = 0xFFFFFFFF
    try:
        kernel32 = ctypes.windll.kernel32
        h = kernel32.OpenProcess(PROCESS_SYNCHRONIZE, False, int(parent_pid))
        if not h:
            # OpenProcess failed (parent already gone? insufficient
            # rights?). Fall back to polling.
            log(f"watchdog: OpenProcess({parent_pid}) failed, polling instead")
            while True:
                time.sleep(3)
                try:
                    os.kill(int(parent_pid), 0)
                except OSError:
                    log(f"watchdog: parent PID {parent_pid} gone (poll); exiting")
                    os._exit(0)
                    return
        # Block until the parent exits.
        kernel32.WaitForSingleObject(h, INFINITE)
        kernel32.CloseHandle(h)
        log(f"watchdog: parent PID {parent_pid} exited; shield exiting")
        os._exit(0)
    except Exception as e:
        log(f"watchdog error: {e}")


def _start_parent_watchdog(parent_pid):
    if not parent_pid:
        return
    t = threading.Thread(target=_parent_watchdog, args=(parent_pid,), daemon=True)
    t.start()
    log(f"watchdog: monitoring parent PID {parent_pid}")

# --- Dependency Imports ---
try:
    import cv2
    import numpy as np
    from PyQt6.QtWidgets import QApplication, QWidget, QLabel, QVBoxLayout
    from PyQt6.QtCore import Qt, QTimer, pyqtSignal, QObject, QThread, QPoint
    from PyQt6.QtGui import QColor, QFont
except ImportError as e:
    # Ensure dependencies are installed by wrapper first
    log(f"Dependency import failed: {e}")
    sys.exit(1)

# --- Configuration & Constants ---
import socket
socket.setdefaulttimeout(15)  # Prevent urlretrieve from hanging forever

MODELS_DIR = os.path.join(os.environ.get("LOCALAPPDATA", ""), "WinCommander", "models")
FACE_MODEL_PATH = os.path.join(MODELS_DIR, "face_landmarker.task")

# EfficientDet-Lite Models (Object Detection)
# Mapping 'nano' -> nano.pt, 'medium' -> medium.pt, etc
OBJ_MODELS = {
    "nano": ("nano.pt", "https://storage.googleapis.com/mediapipe-models/object_detector/efficientdet_lite0/float16/1/efficientdet_lite0.tflite"),
    "small": ("small.pt", "https://storage.googleapis.com/mediapipe-models/object_detector/efficientdet_lite0/float16/1/efficientdet_lite0.tflite"), 
    "medium": ("medium.pt", "https://storage.googleapis.com/mediapipe-models/object_detector/efficientdet_lite2/float16/1/efficientdet_lite2.tflite"),
    "large": ("large.pt", "https://storage.googleapis.com/mediapipe-models/object_detector/efficientdet_lite2/float16/1/efficientdet_lite2.tflite")
}

FACE_MODEL_URL = "https://storage.googleapis.com/mediapipe-models/face_landmarker/face_landmarker/float16/1/face_landmarker.task"
MIN_MODEL_SIZE = 50_000  # 50 KB minimum - guards against empty/corrupt/partial downloads

# --- Windows Blur API ---
def enable_window_blur(hwnd: int, opacity: int = 200) -> bool:
    if sys.platform != "win32": return False
    class ACCENTPOLICY(ctypes.Structure):
        _fields_ = [("AccentState", ctypes.c_int), ("AccentFlags", ctypes.c_int), ("GradientColor", ctypes.c_uint32), ("AnimationId", ctypes.c_int)]
    class WINDOWCOMPOSITIONATTRIBDATA(ctypes.Structure):
        _fields_ = [("Attribute", ctypes.c_int), ("Data", ctypes.c_void_p), ("SizeOfData", ctypes.c_size_t)]
    
    # Calculate GradientColor: (Opacity << 24) | (B << 16) | (G << 8) | R
    # We'll use black tint (0,0,0)
    gradient_color = (opacity << 24) | (0 << 16) | (0 << 8) | 0
    
    # AccentState 3 = BLURBEHIND (Traditional Aero/Acrylic blur)
    # AccentState 4 = ACRYLIC (Modern, uses GradientColor for tint/opacity)
    # We use 4 usually for tint control, but 3 might ignore GradientColor depending on Windows version.
    # Let's try AccentState=3 first, but usually opacity control requires ACCENT_ENABLE_ACRYLICBLURBEHIND (4).
    # However, for pure blur without heavy tint, 3 is standard. But user wants opacity control.
    # Let's use 4 (ACRYLIC) if opacity < 255.
    
    accent = ACCENTPOLICY(AccentState=4, AccentFlags=0, GradientColor=gradient_color, AnimationId=0)
    data = WINDOWCOMPOSITIONATTRIBDATA(Attribute=19, Data=ctypes.addressof(accent), SizeOfData=ctypes.sizeof(accent))
    user32 = ctypes.WinDLL("user32")
    return user32.SetWindowCompositionAttribute(wintypes.HWND(hwnd), ctypes.byref(data)) != 0

def set_process_dpi_aware():
    try: ctypes.windll.shcore.SetProcessDpiAwareness(1)
    except: pass

# --- Worker Thread ---
class ShieldWorker(QThread):
    state_changed = pyqtSignal(bool, str) # (is_clear, reason_msg)
    status_msg = pyqtSignal(str)
    init_failed = pyqtSignal(str)

    def __init__(self, camera_idx=0, check_gaze=True, check_faces=False, check_phone=False,
                 capture_on_device=False,
                 capture_on_multi_face=False,
                 model_level="nano", confidence=0.5, cam_width=640, cam_height=480,
                 wake_delay_ms=150, device_wake_multiplier=2, multi_face_wake_multiplier=2,
                 buffer_frames=2, capture_speed=1):
        super().__init__()
        self.camera_idx = camera_idx
        self.check_gaze = check_gaze
        self.check_faces = check_faces
        self.check_phone = check_phone
        self.capture_on_device = capture_on_device
        self.capture_on_multi_face = capture_on_multi_face
        self.model_level = model_level
        self.confidence = confidence
        self.cam_width = cam_width
        self.cam_height = cam_height
        self.wake_delay_ms = max(50, int(wake_delay_ms))
        self.device_wake_multiplier = max(1, min(20, int(device_wake_multiplier)))
        self.multi_face_wake_multiplier = max(1, min(20, int(multi_face_wake_multiplier)))
        self.buffer_frames = max(1, int(buffer_frames))
        self.capture_speed = max(1, min(4, int(capture_speed)))

        self._running = True
        self._is_locked = True
        
        # Hysteresis timers
        self._attentive_ms = 0.0
        self._distracted_ms = 0.0
        self._lock_reason = ""
        self._captured_for_device = False
        self._captured_for_multi_face = False
        self._device_detected_streak = 0
        self._multi_face_detected_streak = 0
        self._frame_buffer = deque(maxlen=30)
        self._active_recordings = []

        self.mp_face = None
        self.mp_obj = None
        self.face_detector = None
        self.obj_detector = None

    def _is_model_valid(self, path: str) -> bool:
        """Return True only if the file exists and is large enough to be a real model.
        Deletes the file if it exists but is suspiciously small (corrupt / partial download)."""
        if not os.path.exists(path):
            return False
        size = os.path.getsize(path)
        if size < MIN_MODEL_SIZE:
            log(f"Model file is too small ({size} bytes), likely corrupt - deleting for re-download: {path}")
            try:
                os.remove(path)
            except Exception as e:
                log(f"Could not remove corrupt model file: {e}")
            return False
        return True

    def _ensure_models(self):
        os.makedirs(MODELS_DIR, exist_ok=True)
        if (self.check_gaze or self.check_faces) and not self._is_model_valid(FACE_MODEL_PATH):
            try:
                urllib.request.urlretrieve(FACE_MODEL_URL, FACE_MODEL_PATH)
            except Exception as e:
                log(f"Failed to download face model: {e}")
                return False
        
        if self.check_phone:
            # Determine which model to download based on level
            model_info = OBJ_MODELS.get(self.model_level, OBJ_MODELS["nano"])
            filename, url = model_info
            path = os.path.join(MODELS_DIR, filename)
            
            # Store path for loading
            self.obj_model_path = path
            
            if not self._is_model_valid(path):
                try:
                    urllib.request.urlretrieve(url, path)
                except Exception as e:
                    log(f"Failed to download object model ({self.model_level}): {e}")
                    return False
        return True

    def _init_detectors(self):
        try:
            import mediapipe as mp
            from mediapipe.tasks import python
            from mediapipe.tasks.python import vision

            if self.check_gaze or self.check_faces:
                base_options = python.BaseOptions(model_asset_path=FACE_MODEL_PATH)
                options = vision.FaceLandmarkerOptions(
                    base_options=base_options,
                    output_face_blendshapes=True,
                    num_faces=5 if self.check_faces else 1,
                    min_face_detection_confidence=0.5
                )
                self.face_detector = vision.FaceLandmarker.create_from_options(options)

            if self.check_phone:
                base_options = python.BaseOptions(model_asset_path=self.obj_model_path)
                # Max results = 5 objects
                options = vision.ObjectDetectorOptions(
                    base_options=base_options,
                    score_threshold=self.confidence,
                    max_results=5,
                    category_allowlist = ["cell phone", "remote"] 
                )
                self.obj_detector = vision.ObjectDetector.create_from_options(options)
                
            self.mp = mp
            return True
        except Exception as e:
            err_str = str(e)
            if "flatbuffer" in err_str.lower() or "not a valid" in err_str.lower():
                # Cached model files are corrupt - delete them so next start re-downloads cleanly
                log(f"Corrupt model files detected (Flatbuffer error), removing cache for re-download")
                for p in [FACE_MODEL_PATH] + [os.path.join(MODELS_DIR, v[0]) for v in OBJ_MODELS.values()]:
                    if os.path.exists(p):
                        try:
                            os.remove(p)
                            log(f"Removed corrupt model: {p}")
                        except Exception as re:
                            log(f"Could not remove {p}: {re}")
            log(f"Detector init failed: {e}")
            self.status_msg.emit(f"Init Error: {e}")
            return False

    def _check_gaze(self, landmarks) -> bool:
        # Simple heuristic: Nose vs Cheeks
        nose = landmarks[1]
        l_cheek = landmarks[234]
        r_cheek = landmarks[454]
        face_width = r_cheek.x - l_cheek.x
        if face_width <= 0: return False
        mid_x = (l_cheek.x + r_cheek.x) / 2.0
        yaw_ratio = (nose.x - mid_x) / face_width
        return abs(yaw_ratio) <= 0.35 # Slightly looser than before

    def _start_webcam_recording(self, prefix: str, reason_key: str, post_roll_sec: float = 5.0):
        """Start recording webcam. Records while detection is active + post_roll_sec after it ends."""
        try:
            if not self._frame_buffer:
                return
            video_path = os.path.join(CAPTURES_DIR, f"{prefix}_webcam.avi")
            first = self._frame_buffer[0]
            h, w = first.shape[:2]
            fourcc = cv2.VideoWriter_fourcc(*'MJPG')
            output_fps = 30.0 * self.capture_speed
            out = cv2.VideoWriter(video_path, fourcc, output_fps, (w, h))
            if not out.isOpened():
                log(f"VideoWriter failed for {video_path}")
                return
            for f in self._frame_buffer:
                out.write(f)
            now = time.monotonic()
            self._active_recordings.append({
                "out": out, "path": video_path, "reason_key": reason_key,
                "record_until": now + post_roll_sec, "in_post_roll": False
            })
        except Exception as e:
            log(f"Start webcam recording failed: {e}")

    def _capture_incident(self, frame, reason: str, reason_key: str):
        """Capture screen (PNG) + start webcam video. Video records while detection active + 5s post-roll."""
        try:
            os.makedirs(CAPTURES_DIR, exist_ok=True)
            ts = time.strftime("%Y%m%d_%H%M%S")
            prefix = ts + "_" + reason.replace(" ", "_").lower()
            screen_path = os.path.join(CAPTURES_DIR, f"{prefix}_screen.png")
            if sys.platform == "win32":
                from PIL import ImageGrab
                img = ImageGrab.grab()
                img.save(screen_path)
            self._start_webcam_recording(prefix, reason_key)
            log(f"Captured incident: {screen_path}, webcam video (records while active + 5s post)")
        except Exception as e:
            log(f"Capture failed: {e}")

    def run(self):
        if not self._ensure_models():
            log("Model preparation failed")
            self.init_failed.emit("Model preparation failed")
            return
        if not self._init_detectors():
            log("Detector initialization failed")
            self.init_failed.emit("Detector initialization failed")
            return

        # Open the camera in a background thread with a hard timeout.
        # cv2.VideoCapture()+DirectShow can hang indefinitely on headless
        # or no-camera systems (servers, VMs, remote-desktop sessions),
        # which keeps the process alive and makes the UI show "Running"
        # even though no detection ever starts.
        _cam_result = [None]
        _cam_done = threading.Event()
        def _try_open_cam():
            try:
                c = cv2.VideoCapture(self.camera_idx, cv2.CAP_DSHOW)
                if not c.isOpened():
                    c = cv2.VideoCapture(self.camera_idx)
                _cam_result[0] = c
            except Exception as exc:
                log(f"Camera open exception: {exc}")
            finally:
                _cam_done.set()
        threading.Thread(target=_try_open_cam, daemon=True).start()
        if not _cam_done.wait(timeout=5.0):
            log("No camera detected - VideoCapture timed out (no webcam on this system)")
            self.init_failed.emit("No camera detected - this system has no webcam")
            return
        cap = _cam_result[0]
        if not cap or not cap.isOpened():
            self.status_msg.emit("Camera Failed")
            log("No camera detected - VideoCapture returned not-opened")
            self.init_failed.emit("No camera detected - webcam not found or in use by another app")
            return

        self.status_msg.emit("Shield Active")
        
        last_ts = time.monotonic()
        while self._running:
            now_ts = time.monotonic()
            delta_ms = (now_ts - last_ts) * 1000.0
            last_ts = now_ts
            ret, frame = cap.read()
            if not ret:
                time.sleep(0.5)
                continue

            self._frame_buffer.append(frame.copy())

            # Convert frame for MediaPipe
            rgb_frame = cv2.cvtColor(frame, cv2.COLOR_BGR2RGB)
            mp_image = self.mp.Image(image_format=self.mp.ImageFormat.SRGB, data=rgb_frame)
            
            is_clear = True
            reason = ""
            
            try:
                device_triggered = False
                multi_face_triggered = False
                gaze_triggered = False
                no_face_triggered = False

                # 1. Check Phone/Object - require buffer_frames consecutive detections
                if self.check_phone and self.obj_detector:
                    obj_results = self.obj_detector.detect(mp_image)
                    raw_device = any(
                        d.categories[0].category_name in ["cell phone", "remote"]
                        for d in obj_results.detections
                    )
                    if raw_device:
                        self._device_detected_streak += 1
                    else:
                        self._device_detected_streak = 0
                    if self._device_detected_streak >= self.buffer_frames:
                        device_triggered = True

                # 2. Check Faces (Count & Gaze) - always run so we capture for both when both detected
                if (self.check_faces or self.check_gaze) and self.face_detector:
                    face_results = self.face_detector.detect(mp_image)
                    landmarks = face_results.face_landmarks

                    if not landmarks:
                        if self.check_gaze:
                            no_face_triggered = True
                    else:
                        raw_multi = self.check_faces and len(landmarks) > 1
                        if raw_multi:
                            self._multi_face_detected_streak += 1
                        else:
                            self._multi_face_detected_streak = 0
                        if self._multi_face_detected_streak >= self.buffer_frames:
                            multi_face_triggered = True
                        if not multi_face_triggered and self.check_gaze:
                            if not self._check_gaze(landmarks[0]):
                                gaze_triggered = True

                # Combine into is_clear and display reason (priority: device > multi-face > gaze > no-face)
                if device_triggered or multi_face_triggered or gaze_triggered or no_face_triggered:
                    is_clear = False
                    parts = []
                    if device_triggered:
                        parts.append("PHONE DETECTED")
                    if multi_face_triggered:
                        parts.append("MULTIPLE FACES")
                    if gaze_triggered:
                        parts.append("LOOK AWAY")
                    if no_face_triggered:
                        parts.append("NO FACE")
                    reason = " & ".join(parts)

            except Exception as e:
                log(f"Detection loop error: {e}")

            # Capture for each triggered reason (both device and multi-face when both detected)
            if device_triggered and self.capture_on_device and not self._captured_for_device:
                self._capture_incident(frame, "PHONE DETECTED", "device")
                self._captured_for_device = True
            if multi_face_triggered and self.capture_on_multi_face and not self._captured_for_multi_face:
                self._capture_incident(frame, "MULTIPLE FACES", "multi_face")
                self._captured_for_multi_face = True

            # Hysteresis Smoothing
            if is_clear:
                self._attentive_ms += delta_ms
                self._distracted_ms = 0.0
            else:
                self._distracted_ms += delta_ms
                self._attentive_ms = 0.0
            
            # Transition Logic
            if self._is_locked:
                device_in_reason = "PHONE DETECTED" in self._lock_reason
                multi_in_reason = "MULTIPLE FACES" in self._lock_reason
                device_clear = self.wake_delay_ms * self.device_wake_multiplier if device_in_reason else 0
                multi_clear = self.wake_delay_ms * self.multi_face_wake_multiplier if multi_in_reason else 0
                required_clear_ms = max(device_clear, multi_clear, self.wake_delay_ms)

                # A gaze/no-face overlay must disappear on the very first
                # attentive frame.  Applying the general wake hysteresis here
                # left Fleet-started sessions blank after the user looked back.
                # Keep the stricter clear window only when the lock was caused
                # by a phone or multiple faces, where a brief clear frame is
                # not enough to safely reveal the screen.
                clear_immediately = is_clear and not device_in_reason and not multi_in_reason
                if clear_immediately or self._attentive_ms >= required_clear_ms:
                    self._is_locked = False
                    self._lock_reason = ""
                    self._captured_for_device = False
                    self._captured_for_multi_face = False
                    self.state_changed.emit(True, "")
                elif reason and reason != self._lock_reason:
                    # A new condition can arrive while a prior condition is
                    # still active (for example look-away followed by phone
                    # detection). Emit the new reason so its independent blur
                    # toggle and Fleet alert are evaluated immediately.
                    self._lock_reason = reason
                    self.state_changed.emit(False, reason)
            else:
                buffer_ms = self.buffer_frames * 30.0
                if self._distracted_ms >= buffer_ms:
                    self._is_locked = True
                    self._lock_reason = reason
                    self.state_changed.emit(False, reason)

            still_active = []
            for rec in self._active_recordings:
                out, path = rec["out"], rec["path"]
                rk, until, in_post = rec["reason_key"], rec["record_until"], rec["in_post_roll"]
                triggered = (rk == "device" and device_triggered) or (rk == "multi_face" and multi_face_triggered)
                if not in_post:
                    if triggered:
                        rec["record_until"] = now_ts + 5.0
                    else:
                        rec["in_post_roll"] = True
                        rec["record_until"] = now_ts + 5.0
                if now_ts < rec["record_until"]:
                    out.write(frame)
                    still_active.append(rec)
                else:
                    out.release()
                    log(f"Recorded webcam video: {path}")
            self._active_recordings = still_active

            time.sleep(0.03)

        for rec in self._active_recordings:
            try:
                rec["out"].release()
            except Exception:
                pass
        self._active_recordings.clear()
        cap.release()
        if self.face_detector: self.face_detector.close()
        if self.obj_detector: self.obj_detector.close()

# --- UI Overlay ---
class ShieldOverlay(QWidget):
    def __init__(self, screen, opacity=200):
        super().__init__()
        self.setWindowFlags(Qt.WindowType.FramelessWindowHint | Qt.WindowType.WindowStaysOnTopHint | Qt.WindowType.Tool | Qt.WindowType.WindowTransparentForInput | Qt.WindowType.WindowDoesNotAcceptFocus)
        self.setAttribute(Qt.WidgetAttribute.WA_TranslucentBackground)
        self.setAttribute(Qt.WidgetAttribute.WA_TransparentForMouseEvents)
        # One overlay per physical display is reliable on mixed-DPI and
        # negative-coordinate monitor layouts.
        self.setGeometry(screen.geometry())
        
        # Message Label
        self.label = QLabel(self)
        self.label.setAlignment(Qt.AlignmentFlag.AlignCenter)
        self.label.setStyleSheet("color: rgba(255, 50, 50, 200); font-weight: bold; font-family: Consolas; font-size: 48px;")
        self.label.hide()
        
        layout = QVBoxLayout(self)
        layout.addWidget(self.label)
        layout.setAlignment(Qt.AlignmentFlag.AlignCenter)
        
        self.opacity_val = opacity
        self.enable_blur()

    def enable_blur(self):
        QTimer.singleShot(100, lambda: enable_window_blur(int(self.winId()), self.opacity_val))

    def update_state(self, is_clear, reason):
        if is_clear:
            self.hide()
            self.label.hide()
        else:
            self.show()
            self.enable_blur()
            if reason and reason != "NO FACE" and reason != "LOOK AWAY":
                self.label.setText(reason)
                self.label.show()
            else:
                self.label.hide()

class ShieldOverlays:
    def __init__(self, app, opacity=200):
        self.overlays = [ShieldOverlay(screen, opacity) for screen in app.screens()]

    def update_state(self, is_clear, reason):
        for overlay in self.overlays:
            overlay.update_state(is_clear, reason)

# --- Main App ---
class ShieldApp(QObject):
    def __init__(self):
        super().__init__()
        self.app = QApplication(sys.argv)
        set_process_dpi_aware()
        
        parser = argparse.ArgumentParser()
        parser.add_argument('--camera', type=int, default=0)
        parser.add_argument('--check-gaze', action='store_true')
        parser.add_argument('--check-faces', action='store_true')
        parser.add_argument('--check-phone', action='store_true')
        parser.add_argument('--blur-gaze', action='store_true')
        parser.add_argument('--blur-faces', action='store_true')
        parser.add_argument('--blur-phone', action='store_true')
        parser.add_argument('--capture-on-device', action='store_true')
        parser.add_argument('--capture-on-multi-face', action='store_true')
        
        # Granular Controls
        parser.add_argument('--model-level', type=str, default='nano', help='nano, small, medium, large')
        parser.add_argument('--confidence', type=float, default=0.5)
        parser.add_argument('--overlay-opacity', type=int, default=200)
        parser.add_argument('--wake-delay-ms', type=int, default=150)
        parser.add_argument('--device-wake-multiplier', type=int, default=2)
        parser.add_argument('--multi-face-wake-multiplier', type=int, default=2)
        parser.add_argument('--buffer-frames', type=int, default=6)
        parser.add_argument('--capture-speed', type=int, default=1, help='Video playback speed: 1=real-time, 2=2x, 3=3x, 4=4x')
        # Ignored for now but kept for API compat if needed
        parser.add_argument('--mode', type=str, default='')
        # WinCommander process PID -- if set, we start a watchdog thread
        # that self-exits when this PID disappears. Backstop for the
        # Job Object orphan-killer on the Rust side.
        parser.add_argument('--parent-pid', type=int, default=0)

        self.args, _ = parser.parse_known_args()

        # Start the parent-PID watchdog before doing any heavy init so
        # we can never get stuck in an init-blocked state where the
        # only thing holding us alive is our own UI thread.
        if self.args.parent_pid:
            _start_parent_watchdog(self.args.parent_pid)

        log(f"Shield starting: camera={self.args.camera}, gaze={self.args.check_gaze}, faces={self.args.check_faces}, phone={self.args.check_phone}, model={self.args.model_level}, wake_delay_ms={self.args.wake_delay_ms}, device_multiplier={self.args.device_wake_multiplier}, multi_face_multiplier={self.args.multi_face_wake_multiplier}, buffer_frames={self.args.buffer_frames}")

        # L9: retention purge — captures older than 7d are deleted at session start
        try:
            if os.path.isdir(CAPTURES_DIR):
                _now = time.time()
                for _fname in os.listdir(CAPTURES_DIR):
                    _fpath = os.path.join(CAPTURES_DIR, _fname)
                    try:
                        if os.path.isfile(_fpath) and (_now - os.stat(_fpath).st_mtime) > 7 * 86400:
                            os.remove(_fpath)
                    except Exception:
                        pass
        except Exception:
            pass

        # Defaults if no flags set (default to basic eye tracker behavior)
        if not (self.args.check_gaze or self.args.check_faces or self.args.check_phone):
            self.args.check_gaze = True

        # Fresh event sidecar per session so the Rust reader never replays
        # stale look-away/look-back transitions from a previous run.
        try:
            with open(EVENTS_FILE, "w", encoding="utf-8") as _ef:
                _ef.write("")
        except Exception:
            pass

        self.overlay = ShieldOverlays(self.app, opacity=self.args.overlay_opacity)
        # Pass new configurations
        self.worker = ShieldWorker(
            camera_idx=self.args.camera, 
            check_gaze=self.args.check_gaze, 
            check_faces=self.args.check_faces, 
            check_phone=self.args.check_phone,
            capture_on_device=self.args.capture_on_device,
            capture_on_multi_face=self.args.capture_on_multi_face,
            model_level=self.args.model_level,
            confidence=self.args.confidence,
            wake_delay_ms=self.args.wake_delay_ms,
            device_wake_multiplier=self.args.device_wake_multiplier,
            multi_face_wake_multiplier=self.args.multi_face_wake_multiplier,
            buffer_frames=self.args.buffer_frames,
            capture_speed=max(1, min(4, self.args.capture_speed))
        )
        self.worker.state_changed.connect(self._handle_state)
        self.worker.status_msg.connect(lambda msg: log(f"Status: {msg}"))
        self.worker.init_failed.connect(self.handle_init_fail)
        self.worker.start()
        log("Shield worker started")

    def _handle_state(self, is_clear, reason):
        # Fleet/flow receives every transition. The local blur toggles only
        # control visual enforcement; an unchecked toggle never silences an
        # attention event.
        self._emit_look_event(is_clear, reason)
        should_blur = is_clear or (
            (("LOOK AWAY" in reason) or ("NO FACE" in reason)) and self.args.blur_gaze
        ) or (("MULTIPLE FACES" in reason) and self.args.blur_faces) or (
            ("PHONE DETECTED" in reason) and self.args.blur_phone
        )
        self.overlay.update_state(is_clear or not should_blur, reason)

    def _emit_look_event(self, is_clear, reason):
        # Qt look-state change -> NDJSON sidecar. `is_clear` True means the
        # user is present/attentive (look_back); False means the overlay
        # engaged (look_away), carrying the trigger reason for context.
        emit_event("look_back" if is_clear else "look_away", "" if is_clear else reason)

    def handle_init_fail(self, msg):
        log(f"Shield init failed: {msg}")
        self.app.quit()
        sys.exit(1)
        
    def run(self):
        sys.exit(self.app.exec())

if __name__ == "__main__":
    try:
        ShieldApp().run()
    except Exception:
        log(traceback.format_exc())
'@
        # Secure cleanup of legacy plaintext copies from previous builds.
        if ($env:LOCALAPPDATA) {
            Remove-ItemSecure -Path (Join-Path $env:LOCALAPPDATA "WinCommander\privacy_shield.py") -Force -ErrorAction SilentlyContinue
        }
        if ($env:APPDATA) {
            Remove-ItemSecure -Path (Join-Path $env:APPDATA "WinCommander\privacy_shield.py") -Force -ErrorAction SilentlyContinue
        }

        # The PowerShell host that's running this script was spawned
        # directly by WinCommander (backend.rs -> std::process::Command),
        # so this process's parent IS the WinCommander PID we want the
        # Python watchdog to track.
        $wcPid = 0
        try {
            $wcPid = (Get-CimInstance Win32_Process -Filter "ProcessId=$PID" `
                -ErrorAction SilentlyContinue).ParentProcessId
        } catch {}
        if (-not $wcPid) { $wcPid = 0 }

        $pythonArgs = @(
            "-",
            "$shieldProcessMarker",
            "--camera", "$Camera",
            "--model-level", "$ModelLevel",
            "--confidence", "$Confidence",
            "--overlay-opacity", "$OverlayOpacity",
            "--wake-delay-ms", "$WakeDelayMs",
            "--device-wake-multiplier", "$DeviceWakeMultiplier",
            "--multi-face-wake-multiplier", "$MultiFaceWakeMultiplier",
            "--buffer-frames", "$BufferFrames",
            "--capture-speed", "$CaptureSpeed",
            "--parent-pid", "$wcPid"
        )
        if ($CheckGaze) { $pythonArgs += "--check-gaze" }
        if ($CheckFaces) { $pythonArgs += "--check-faces" }
        if ($CheckPhone) { $pythonArgs += "--check-phone" }
        if ($BlurGaze) { $pythonArgs += "--blur-gaze" }
        if ($BlurFaces) { $pythonArgs += "--blur-faces" }
        if ($BlurPhone) { $pythonArgs += "--blur-phone" }
        if ($CaptureOnDevice) { $pythonArgs += "--capture-on-device" }
        if ($CaptureOnMultiFace) { $pythonArgs += "--capture-on-multi-face" }

        # Use resolved python executable (must be python.exe, NOT pythonw.exe)
        # pythonw.exe often blocks OpenCV/Media Foundation from connecting to the camera.
        $pythonCmd = $pythonExe

        $startInfo = New-Object System.Diagnostics.ProcessStartInfo
        $startInfo.FileName = $pythonCmd
        $startInfo.Arguments = $pythonArgs -join " "
        $startInfo.UseShellExecute = $false
        $startInfo.CreateNoWindow = $true
        $startInfo.RedirectStandardInput = $true
        
        $process = [System.Diagnostics.Process]::Start($startInfo)
        if ($process -and -not $process.HasExited) {
            $process.StandardInput.Write($embeddedScript)
            $process.StandardInput.Close()
        }
        # Poll for up to 8 s. Camera probe inside Python times out at 5 s,
        # so a no-camera exit arrives around 6-7 s. Fast failures (import
        # error, dependency missing) exit within 1-2 s.
        $sw = [System.Diagnostics.Stopwatch]::StartNew()
        $exitedEarly = $false
        while ($sw.ElapsedMilliseconds -lt 8000) {
            Start-Sleep -Milliseconds 300
            if ($process.HasExited) { $exitedEarly = $true; break }
        }

        if ($exitedEarly) {
            $logRoot = if ($env:LOCALAPPDATA) { $env:LOCALAPPDATA } else { $env:TEMP }
            $logPath = if ($logRoot) { Join-Path $logRoot "WinCommander\logs\privacy_shield.log" } else { $null }
            $logTail = ""
            $exitMessage = "Failed to start Privacy Shield - process exited unexpectedly."
            if ($logPath -and (Test-Path $logPath)) {
                $logTail = (Get-Content $logPath -Tail 30 | Out-String).Trim()
                if ($logTail -imatch "no camera|camera open|webcam|videocapture|camera not|camera timed out") {
                    $exitMessage = "No camera detected - Privacy Shield requires a webcam."
                } elseif ($logTail -imatch "missing python dependency|importerror|module not found") {
                    $exitMessage = "Missing Python dependency - please reinstall the AI runtime."
                } elseif ($logTail -imatch "model preparation|failed to download|model download") {
                    $exitMessage = "Model download failed - check your internet connection."
                } elseif ($logTail -imatch "detector init|flatbuffer|not a valid") {
                    $exitMessage = "Model file is corrupt - restart to trigger a re-download."
                }
            }
            return @{ error = $true; message = $exitMessage; debugInfo = $logTail }
        }

        @{ success = $true; processId = $process.Id }
    }
    catch {
        @{ error = $true; message = $_.Exception.Message }
    }
}

function Stop-PrivacyShield {
    try {
        $shieldProcessMarker = "--wc-privacy-shield"
        $status = Get-PrivacyShieldStatus
        if ($status.running) {
            Stop-Process -Id $status.processId -Force -ErrorAction SilentlyContinue
        }
        
        # Cleanup any stragglers
        Get-Process -Name "pythonw", "python" -ErrorAction SilentlyContinue | Where-Object { $_.CommandLine -like "*$shieldProcessMarker*" } | Stop-Process -Force -ErrorAction SilentlyContinue

        return @{ success = $true; message = "Stopped." }
    }
    catch {
        @{ error = $true; message = $_.Exception.Message }
    }
}
