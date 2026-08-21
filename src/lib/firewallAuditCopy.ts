// User-facing copy for the firewall audit — error guidance, confirmation
// consequences, and the backup-path helper. Split from firewallAuditView.ts so
// each file keeps one responsibility: that one reads the data, this one words it.
// No React, no IPC — unit tested in firewallAuditView.test.ts.
import type { ErrorAdvice } from "./arpDiagnostics";
import type { FirewallAction } from "./firewallAuditView";


/** Maps the raw Rust error strings from firewall_audit.rs onto guidance. */
export function classifyFirewallError(raw: string): ErrorAdvice {
  const text = raw.toLowerCase();
  if (text.includes("unsupported on this windows locale")) {
    return {
      title: "Can't read the rule list in this Windows display language",
      hint: "WinCommander only parses netsh output in English so far. Switch the Windows display language, or use Windows Defender Firewall directly.",
      needsElevation: false,
    };
  }
  if (text.includes("expired")) {
    return {
      title: "This audit is too old to act on",
      hint: "Run the audit again — Windows rules can change underneath a stale selection, so anything older than 10 minutes is rejected.",
      needsElevation: false,
    };
  }
  if (text.includes("stale or invalid")) {
    return {
      title: "The selection no longer matches the live rules",
      hint: "Something changed a rule since the audit. Run the audit again and reselect.",
      needsElevation: false,
    };
  }
  if (text.includes("decoy mode")) {
    return {
      title: "Unavailable in Decoy mode",
      hint: "Decoy mode hides and blocks firewall inspection. Leave Decoy mode to use the audit.",
      needsElevation: false,
    };
  }
  if (text.includes("investigator mode")) {
    return {
      title: "Blocked by investigator mode",
      hint: "Investigator mode never changes firewall rules because that alters evidence. Auditing is still allowed.",
      needsElevation: false,
    };
  }
  if (text.includes("access is denied") || text.includes("requires elevation") || text.includes("administrator")) {
    return {
      title: "Windows refused the change",
      hint: "Changing firewall rules needs an elevated process. Restart WinCommander as Administrator and try again.",
      needsElevation: true,
    };
  }
  if (text.includes("invalid firewall rule selection")) {
    return {
      title: "Selection rejected",
      hint: "At most 300 rules can be changed in one pass. Narrow the selection and try again.",
      needsElevation: false,
    };
  }
  return { title: "Firewall audit failed", hint: raw, needsElevation: false };
}

/** Consequence bullets for the remove confirmation. Names are truncated so a
 *  300-rule selection can't produce an unreadable dialog. */
export function firewallRemoveConsequences(ruleNames: string[], maxNames = 6): string[] {
  const shown = ruleNames.slice(0, maxNames);
  const hidden = ruleNames.length - shown.length;
  return [
    `${ruleNames.length} firewall rule${ruleNames.length === 1 ? "" : "s"} ${ruleNames.length === 1 ? "is" : "are"} deleted from Windows Defender Firewall`,
    ...shown.map((name) => `• ${name}`),
    ...(hidden > 0 ? [`• …and ${hidden} more`] : []),
    "Windows cannot undo this — the rules must be recreated by hand or restored from the backup",
    "A full firewall backup (.wfw) is exported first, so the whole rule set can be restored with: netsh advfirewall import <file>",
  ];
}

export interface FirewallConfirmCopy {
  title: string;
  intro: string;
  consequences: string[];
  actionLabel: string;
  /** Only the irreversible action gets an enforced pause. */
  countdownSeconds: number;
}

const ACTION_INTRO: Record<FirewallAction, string> = {
  enable: "Turning a rule back on is reversible — you can disable it again at any time.",
  disable: "Disabling a rule is reversible — the rule stays in Windows and can be re-enabled.",
  remove: "Removing a rule deletes it from Windows Defender Firewall for good.",
};

/** Everything the confirmation dialog needs, derived here so the card stays
 *  presentation-only and the copy is unit-testable. */
export function firewallConfirmCopy(
  action: FirewallAction,
  ruleNames: string[],
  maxNames = 6,
): FirewallConfirmCopy {
  const count = ruleNames.length;
  const noun = `${count} firewall rule${count === 1 ? "" : "s"}`;
  if (action === "remove") {
    return {
      title: `Remove ${noun}?`,
      intro: ACTION_INTRO.remove,
      consequences: firewallRemoveConsequences(ruleNames, maxNames),
      actionLabel: "Remove rules",
      countdownSeconds: 3,
    };
  }
  const verb = action === "enable" ? "Enable" : "Disable";
  const state = action === "enable" ? "on" : "off";
  const hidden = count - Math.min(count, maxNames);
  return {
    title: `${verb} ${noun}?`,
    intro: ACTION_INTRO[action],
    consequences: [
      ...ruleNames.slice(0, maxNames).map((name) => `• ${name} is turned ${state}`),
      ...(hidden > 0 ? [`• …and ${hidden} more`] : []),
      "A full firewall backup (.wfw) is exported first",
      "You can reverse this from this same card",
    ],
    actionLabel: `${verb} rules`,
    countdownSeconds: 0,
  };
}

/** Parent directory of the exported .wfw backup — what we hand to `open_path`,
 *  since opening the .wfw itself would just hit "no associated program". */
export function backupFolderOf(backupPath: string): string {
  const cut = Math.max(backupPath.lastIndexOf("\\"), backupPath.lastIndexOf("/"));
  return cut > 0 ? backupPath.slice(0, cut) : backupPath;
}
