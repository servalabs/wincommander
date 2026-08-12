import { Camera, CameraOff, Mic, MicOff, Globe, GlobeLock, VideoOff } from "lucide-react";
import RadarControlStrip from "./RadarControlStrip";

interface PrivacyTogglesCardProps {
  cameraBlocked?: boolean;
  /** When false, no camera hardware is present — shows a disabled "No Camera" button. */
  cameraAvailable?: boolean;
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
  cameraBlocked = false,
  cameraAvailable = true,
  microphoneBlocked = false,
  capabilityPending = null,
  onCapabilityToggle,
  internetCut = false,
  internetPending = false,
  onToggleInternet,
}: PrivacyTogglesCardProps) {
  return (
    <div className="privacy-toggles-card">
      <div className="ptc-toggles">
        <div className="ptc-title">PRIVACY</div>
        <div className="dashboard-capability-toggles">
          {cameraAvailable ? (
            <button
              type="button"
              className={`dashboard-capability-toggle ${cameraBlocked ? "active" : ""}`}
              onClick={() => onCapabilityToggle?.("webcam", !cameraBlocked)}
              disabled={!onCapabilityToggle || capabilityPending === "webcam"}
              title={cameraBlocked ? "Camera blocked — click to allow" : "Camera allowed — click to block"}
            >
              {cameraBlocked ? <CameraOff size={12} /> : <Camera size={12} />}
              CAM {cameraBlocked ? "OFF" : "ON"}
            </button>
          ) : (
            <button
              type="button"
              className="dashboard-capability-toggle no-device"
              disabled
              title="No camera detected on this device"
            >
              <VideoOff size={12} />
              NO CAM
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
        {onToggleInternet && (
          <button
            type="button"
            className={`killswitch-toggle ${internetCut ? "cut" : "live"}`}
            onClick={() => onToggleInternet(!internetCut)}
            disabled={internetPending}
            title={internetCut
              ? "All internet traffic is blocked — click to restore"
              : "Internet is live — click to cut all traffic"}
          >
            {internetCut ? <GlobeLock size={13} /> : <Globe size={13} />}
            {internetCut ? "Internet OFF" : "Internet ON"}
          </button>
        )}
      </div>
      <RadarControlStrip />
    </div>
  );
}
