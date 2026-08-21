// Pure formatting for an EraseReceipt. No React, no IPC — unit tested in
// cryptoEraseReceipt.test.ts.
//
// WHY: selective_erase.rs works hard to produce honest verification data
// (independent header-byte re-read, an escrow honesty invariant, an FVEK
// eviction retry) and nine receipt fields carry it. The UI previously rendered
// exactly one of them (`detail`) as a 12px sentence, so the single field that
// tells a user their key is escrowed and must be revoked elsewhere was
// invisible. Everything here exists to get those fields on screen as facts.
import type { EraseReceipt } from "../hooks/useBackend";

export type ReceiptTone = "success" | "warning" | "danger";

export function receiptTone(status: EraseReceipt["status"]): ReceiptTone {
  if (status === "erased") return "success";
  if (status === "erased_with_caveat") return "warning";
  return "danger";
}

export function receiptHeadline(status: EraseReceipt["status"]): string {
  if (status === "erased") return "Keys destroyed";
  if (status === "erased_with_caveat") return "Erased — but recovery is still possible";
  return "Erase failed";
}

export interface ReceiptFact {
  label: string;
  value: string;
  tone: ReceiptTone | "neutral";
  /** Non-obvious enough to earn an info tooltip. */
  tooltip: string;
}

const ACTION_LABEL: Record<string, string> = {
  veracrypt_header_destroy: "VeraCrypt header destroy",
  bitlocker_protectors_removed: "BitLocker protector removal",
};

/** The structured facts that used to be thrown away. Rendered as a small grid
 *  under the status pill. */
export function receiptFacts(receipt: EraseReceipt): ReceiptFact[] {
  const facts: ReceiptFact[] = [
    {
      label: "Method",
      value: ACTION_LABEL[receipt.action] ?? receipt.action,
      tone: "neutral",
      tooltip:
        "Which destruction path ran. VeraCrypt overwrites the volume header; BitLocker strips the key protectors that unwrap the volume key.",
    },
    {
      label: "Verified",
      value: receipt.verified ? "Yes" : "No",
      tone: receipt.verified ? "success" : "warning",
      tooltip:
        "Yes means WinCommander independently re-read the volume after the erase and confirmed the change — not just that the command reported success.",
    },
  ];

  if (receipt.kind === "bitlocker") {
    facts.push({
      label: "Key evicted",
      value: receipt.keyEvicted ? "Yes" : "No",
      tone: receipt.keyEvicted ? "success" : "warning",
      tooltip:
        "Whether the volume was locked so Windows dropped its decryption key from memory. Until that happens the volume stays readable in this session.",
    });
    const remaining = receipt.recoveryProtectorsRemaining;
    if (remaining != null) {
      facts.push({
        label: "Recovery keys left",
        value: String(remaining),
        tone: remaining === 0 ? "success" : "danger",
        tooltip:
          "Any protector still attached to the volume can unlock it. This must be 0 for the erase to be final.",
      });
    }
  }

  return facts;
}

/** The one thing the user has to go and do somewhere else. Returns null when
 *  there is nothing outstanding. */
export function receiptEscrowAdvice(receipt: EraseReceipt): string | null {
  const remaining = receipt.recoveryProtectorsRemaining ?? 0;
  if (!receipt.escrowWarning && remaining <= 0) return null;
  const detail = receipt.escrowWarning?.trim();
  const lead = detail ? `${detail} ` : "";
  return `${lead}This volume can still be unlocked with its recovery key. Sign in to Entra/Active Directory (or account.microsoft.com for a personal account), delete the saved recovery key for ${receipt.label}, and treat the data as recoverable until you have.`;
}

/** Plain-text receipt for an audit trail. `at` is a millisecond timestamp so
 *  callers stay testable (no implicit Date.now inside). */
export function formatReceiptForClipboard(receipt: EraseReceipt, at: number): string {
  const lines = [
    "WinCommander crypto-erase receipt",
    `Time: ${new Date(at).toISOString()}`,
    `Target: ${receipt.label}`,
    `Kind: ${receipt.kind}`,
    `Method: ${ACTION_LABEL[receipt.action] ?? receipt.action}`,
    `Status: ${receipt.status}`,
    `Independently verified: ${receipt.verified ? "yes" : "no"}`,
    `Key evicted: ${receipt.keyEvicted ? "yes" : "no"}`,
  ];
  if (receipt.recoveryProtectorsRemaining != null) {
    lines.push(`Recovery protectors remaining: ${receipt.recoveryProtectorsRemaining}`);
  }
  if (receipt.escrowWarning) lines.push(`Escrow warning: ${receipt.escrowWarning}`);
  lines.push(`Detail: ${receipt.detail}`);
  return lines.join("\n");
}

/** Maps the raw Rust refusal strings from selective_erase.rs onto guidance for
 *  the inline dialog error, so a failure is never just a bare exception string. */
export function classifyEraseError(raw: string): { title: string; hint: string } {
  const text = raw.toLowerCase();
  if (text.includes("investigator mode")) {
    return {
      title: "Blocked by investigator mode",
      hint: "Investigator mode never destroys keys on a seized device. Leave investigator mode to use crypto-erase.",
    };
  }
  if (text.includes("typed confirmation matching")) {
    return {
      title: "Confirmation did not match",
      hint: raw,
    };
  }
  if (text.includes("not confirmed")) {
    return {
      title: "Confirmation missing",
      hint: "The erase was sent without a confirmation flag. Close this dialog and try again.",
    };
  }
  if (text.includes("requires a path")) {
    return {
      title: "Container path unknown",
      hint: "WinCommander could not tell which file backs this volume. Dismount it and re-mount it from its container file, then retry.",
    };
  }
  if (text.includes("licen") || text.includes("pro ") || text.includes("paid")) {
    return {
      title: "Crypto-erase needs WinCommander Pro",
      hint: "The destruction handlers ship with the Pro engine. Activate Pro to use this feature.",
    };
  }
  return { title: "Crypto-erase failed", hint: raw };
}
