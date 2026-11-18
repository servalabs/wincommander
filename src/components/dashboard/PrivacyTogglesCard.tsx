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
  return (
    <div className="privacy-toggles-card">
      <div className="ptc-toggles">
        <div className="ptc-title">PRIVACY</div>
        <div className="dashboard-capability-toggles">
          {cameraState === "unknown" ? (
            <button type="button" className="dashboard-capability-toggle no-device" disabled
              title="Camera access is being checked">
              <Camera size={12} /> CAM ?
            </button>
          ) : (
            <button
              type="button"
              className={`dashboard-capability-toggle ${cameraBlocked ? "active" : ""}`}
              onClick={() => onCapabilityToggle?.("webcam", !cameraBlocked)}
              disabled={!onCapabilityToggle || capabilityPending === "webcam"}
              title={`${cameraBlocked ? "Camera access blocked - click to allow" : cameraMixed ? "Camera permissions differ between apps - click to block" : "Camera access allowed - click to block"}${cameraPresence === "absent" ? "; no camera detected" : cameraPresence === "unknown" ? "; camera presence could not be confirmed" : ""}`}
            >
              {cameraBlocked ? <CameraOff size={12} /> : <Camera size={12} />}
              CAM {cameraBlocked ? "OFF" : cameraMixed ? "MIXED" : "ON"}
              {cameraPresence === "absent" && <span> - NO CAM</span>}
            </button>
          )}
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
