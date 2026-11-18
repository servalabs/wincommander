import { Camera, CameraOff, Mic, MicOff, Globe, GlobeLock } from "lucide-react";
import RadarControlStrip from "./RadarControlStrip";
import BluetoothControl from "./BluetoothControl";
import { cameraDisplayState, type CameraPresence } from "../../hooks/cameraPresenceProbe";

interface PrivacyTogglesCardProps {
  cameraBlocked?: boolean | null;
  cameraPresence?: CameraPresence;
  cameraMixed?: boolean;
  microphoneBlocked?: boolean;
  capabilityPending?: "webcam" | "microphone" | null;
  onCapabilityToggle?: (capability: "webcam" | "microphone", blocked: boolean) => void;
  /** Internet kill switch: true = all traffic cut (firewall block rules active). */
  internetCut?: boolean;
  /** True while a kill-switch toggle is in flight (disables the button). */
  internetPending?: boolean;
  onToggleInternet?: (cut: boolean) => void;
}

/**
 * PrivacyTogglesCard — the dedicated Camera / Microphone / Internet kill-switch
 * toggles, on the left column of the dashboard (owner request: separate toggles
 * on the left; Public IP + DNS readouts live near the radar). Each is its own
 * independent toggle.
 */
export default function PrivacyTogglesCard({
  cameraBlocked = null,
  cameraPresence = "unknown",
  cameraMixed = false,
  microphoneBlocked = false,
  capabilityPending = null,
  onCapabilityToggle,
  internetCut = false,
  internetPending = false,
  onToggleInternet,
}: PrivacyTogglesCardProps) {
  const cameraState = cameraDisplayState(cameraPresence, cameraBlocked);
  const canToggleCamera = (cameraState === "allowed" || cameraState === "blocked")
    && onCapabilityToggle !== undefined && capabilityPending !== "webcam";
  const cameraLabel = { absent: "NO CAM", unknown: "CAM", allowed: "CAM ON", blocked: "CAM OFF" }[cameraState];
  const cameraDescription = {
    absent: cameraPresence === "host_absent"
      ? "No camera detected on this computer. Windows could not verify redirected cameras."
      : "No camera detected.",
    unknown: cameraPresence === "unknown" ? "Camera presence could not be confirmed." : "Camera access could not be confirmed.",
    allowed: cameraMixed ? "Camera access is on. Permissions may differ by app." : "Camera access is on.",
    blocked: "Camera access is off.",
  }[cameraState];
  return (
    <div className="privacy-toggles-card">
      <div className="ptc-toggles">
        <div className="ptc-title">PRIVACY</div>
        <div className="dashboard-capability-toggles">
          <button
            type="button"
            className={`dashboard-capability-toggle whitespace-nowrap ${cameraState === "blocked" ? "active" : cameraState === "absent" ? "no-device" : ""}`}
            onClick={canToggleCamera ? () => onCapabilityToggle?.("webcam", cameraState === "allowed") : undefined}
            disabled={!canToggleCamera}
            title={cameraDescription}
          >
            {cameraState === "blocked" || cameraState === "absent" ? <CameraOff size={12} /> : <Camera size={12} />}
            {cameraLabel}
          </button>
          <button
            type="button"
            className={`dashboard-capability-toggle ${microphoneBlocked ? "active" : ""}`}
            onClick={() => onCapabilityToggle?.("microphone", !microphoneBlocked)}
            disabled={!onCapabilityToggle || capabilityPending === "microphone"}
            title={microphoneBlocked ? "Microphone blocked — click to allow" : "Microphone allowed — click to block"}
          >
            {microphoneBlocked ? <MicOff size={12} /> : <Mic size={12} />}
            MIC {microphoneBlocked ? "OFF" : "ON"}
          </button>
        </div>
        <div className="privacy-connectivity-toggles">
        {onToggleInternet && (
          <button
            type="button"
            className={`killswitch-toggle ${internetCut ? "cut" : "live"}`}
            onClick={() => onToggleInternet(!internetCut)}
            disabled={internetPending}
            aria-label={internetCut ? "Internet blocked" : "Internet on"}
            aria-pressed={internetCut}
            title={internetCut
              ? "All internet traffic is blocked — click to restore"
              : "Internet is live — click to cut all traffic"}
          >
            {internetCut ? <GlobeLock size={13} /> : <Globe size={13} />}
            NET {internetCut ? "OFF" : "ON"}
          </button>
        )}
        <BluetoothControl />
        </div>
      </div>
      <RadarControlStrip />
    </div>
  );
}
