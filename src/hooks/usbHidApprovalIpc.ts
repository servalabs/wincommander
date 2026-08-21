/**
 * Typed public IPC seam for the paid USB HID approval gate. Keep raw Tauri
 * transport out of context/components so the UI consumes only the narrow wire
 * contract and never handles a PnP instance ID.
 */
import { invoke } from "@tauri-apps/api/core";
import type {
  UsbHidApprovalGateStatus,
  UsbHidApprovalList,
  UsbHidVisualChallenge,
  UsbHidVisualChallengeDigitResult,
} from "../lib/usbHidApproval";

export const usbHidApprovalIpc = {
  status: () => invoke<UsbHidApprovalGateStatus>("usb_hid_approval_gate_status"),
  list: () => invoke<UsbHidApprovalList>("get_usb_hid_pending_approvals"),
  start: (approvalTtlSecs: number) => invoke("start_usb_hid_approval_gate", { approvalTtlSecs }),
  stop: () => invoke("stop_usb_hid_approval_gate"),
  keepBlocked: (deviceKey: string) => invoke("block_usb_hid_pending", { deviceKey }),
  beginChallenge: (deviceKey: string, action: UsbHidVisualChallenge["action"]) => (
    invoke<UsbHidVisualChallenge>("begin_usb_hid_visual_challenge", { deviceKey, action })
  ),
  submitChallengeDigit: (deviceKey: string, challengeId: string, step: number, digit: string) => (
    invoke<UsbHidVisualChallengeDigitResult>("submit_usb_hid_visual_challenge_digit", {
      deviceKey,
      challengeId,
      step,
      digit,
    })
  ),
  revealSecurityAlert: () => invoke("reveal_main_window_for_security_alert"),
};
