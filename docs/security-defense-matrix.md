# Security defense and tier matrix

This is the product boundary for WinCommander 3.2.18. “Detect” means an alert
or finding. “Contain” means a reversible response after Windows observes the
event. Neither word implies immunity from an attack.

## Everyday safety kept in Free

| Threat or control | Free behavior | Pro upgrade |
|---|---|---|
| Mass file changes / ransomware | Local folder watcher raises a bounded alarm; threshold, rolling window, cooldown, and watched folders are machine settings. | ETW process attribution, evidence floor, Monitor/Suspend/Kill response, and Fleet reporting. |
| USB device arrival | Machine-wide attach/detach timeline, VID/PID/name, mounted-volume display, and optional local notification. | Device intelligence, transfer metering, policy, and response below. |
| Clipboard credential, card, crypto-address, and ClickFix patterns | Local warning, snooze, and clear controls. | Managed Clipboard Guard rules and content-free Fleet events. |
| VPN disconnect | Local internet kill switch after a genuine up-to-down transition. | Fleet policy/visibility where configured. |
| Windows security baseline | Microsoft Defender/ASR/CFA/Network Protection, exploit protection, AutoPlay, UAC, SmartScreen, VBS, BitLocker, SMBv1/WDigest/NTLMv1, firewall, DNS, and recovery-safe hardening remain available to ordinary users. | Central policy lock, drift evidence, and Fleet reporting. |

## Organisation and defense intelligence in Pro

| Threat or control | Pro behavior | Main operator settings |
|---|---|---|
| USB HID timing anomaly | Records arrival timing only and alerts on sustained super-human bursts after a recent unallowlisted HID attach. Correlation is low-confidence because the Windows low-level hook does not identify the source keyboard. Timing never directly enforces. | Lenient/Balanced/Strict; device allow-list; saved arm state. |
| Unknown USB storage/HID | Observe or reactively quarantine a newly observed, unapproved device after the PnP poll. HID action is separately confirmed. | Off/Observe/Enforce; allow device/vendor; Include HID. |
| USB data movement | Per-device removable-volume read/write totals and large-transfer signals. | Arm state, sample cadence, threshold, alert switch. |
| USB trust and device control | Combines identity, vendor, timing, transfer, and containment history; validates USB/HID PnP targets before Block/Allow/Read-only/Quarantine. Failed Windows commands are reported as failures. | Allow/default policy; block/allow; volume read-only. |
| Decoy-file access | Watches enrolled files for modify/rename/remove; optional Security Log 4663 read/open attribution; local actor context and path-free Fleet tripwire. | Enrolled files, read audit, Fleet reporting, clear history. |
| Failed-login burst | Security Log 4625 burst detection per account. | Threshold, window, repeat debounce, Fleet reporting. |
| Off-hours login and RDP | Evaluates working days, normal/overnight hours, local or UTC basis; RDP carries schedule context. | Working days/hours/time basis; RDP/new-account/off-hours switches. |
| Rogue Wi-Fi / authentication downgrade | Learns a device-local SSID/BSSID/auth baseline and alerts on unexpected AP or weaker authentication. | Learning window, poll cadence, cooldown, baseline reset, Fleet reporting. |
| Vulnerable kernel driver / BYOVD | Compares loaded drivers with the curated vulnerable-driver catalogue and can watch continuously. | Watch cadence, local/Fleet alert settings. |
| Remote-control session | Detects active RDP and supported remote-support sessions rather than mere installation. | Per-tool switches, start/rearm, alert history. |
| Screen-capture tooling | Watches for supported capture tools and can exclude WinCommander’s window from capture. | Detection, own-window protection, Fleet reporting. |
| Malware | Pro scan engine, allow-list, machine-wide quarantine, restore/delete, and findings. | Scan scope, allow-list, quarantine actions. |
| Network reconnaissance | Port Guard/honeypot listener and recent connection evidence. | Ports, bind policy, cooldown/reporting. |
| Canary tokens | Beacon listener and generated canary artifacts with debounced hit history. | Token type, listener, artifact lifecycle. |
| Ransomware automatic response | Attributes a sufficiently evidenced file writer and optionally suspends or terminates it. | Evidence floor and Monitor/Suspend/Kill. |
| DLP / print / removable-media intelligence | Aggregate clipboard, cloud-upload, removable-copy, print, and large-transfer signals without sending content in the Argus wire. | Collector switches and Fleet policy. |
| Tamper and session assurance | Aggregate tamper/evasion and attention/session posture for managed devices. | Collector policy, local status, Fleet policy. |
| Disposable analysis | Hyper-V VM lifecycle and Windows Sandbox where the OS supports them. | VM/sandbox lifecycle and capability checks. |
| Evidence and investigation | Machine signing identity, evidence chain/export, Fleet incident hand-off, and separate Investigator entitlement. | Export/verify, TPM/RFC-3161 options, Fleet workflow. |

## Paid features still hosted in Free for safety or bootstrap reasons

These remain entitlement-gated but are not physically moved in this release:

- Startup calculator/PIN and decoy/destroy verification must run before the Pro
  sidecar can be trusted or launched.
- Lockdown/distress keyboard phrases share the app’s global keyboard-hook and
  decoy-session lifecycle; moving them needs a durable service-owned hook.
- The F6 destructive wipe/boot chain is hardware- and recovery-coupled. It needs
  a separate signed-artifact migration and destructive-device acceptance run.
- Dead-man and inactivity actions need a service lifecycle that can re-arm
  safely without depending on an interactive user session.
- The dashboard’s basic network/CPU/RAM sampling stays Free; only organisation
  alert policy and Fleet reporting belong in Pro.

## Explicit limits and acceptance work

- USB isolation is reactive post-attach containment, not a kernel/pre-mount
  block. It cannot guarantee protection before the first injected key or first
  device access.
- HID timing does not authenticate a Flipper, Rubber Ducky, O.MG cable, or any
  other device. Delayed/jittered scripts, mouse-only attacks, spoofed identity,
  network-capable cables, DMA/Thunderbolt, and preboot input remain outside it.
- Physical malicious-USB acceptance is still required on a disposable Windows
  11 host with composite HID+storage, Flipper Zero, Rubber Ducky, and O.MG.
- Windows Server 2019/2022/2025 Desktop Experience is the GUI target. Server
  Core has no Explorer/taskbar and is not a full WinCommander GUI target.
- Static/build tests do not replace live Server/RDS, elevated ETW, Wi-Fi, RDP,
  malware-quarantine, or physical-device tests.

The durable product rule is simple: Free provides ordinary-person alarms and
Microsoft baseline hardening; Pro provides history, attribution, intelligence,
automatic containment, organisation policy, evidence, and Fleet reporting.
