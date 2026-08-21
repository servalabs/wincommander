# Unknown-keyboard approval

WinCommander Pro can reactively hold a newly observed, unapproved keyboard and
ask an operator what to do. Think of it as moving a visitor back into a lobby
after Windows has already opened the front door: it adds a decision point, but
it is not a pre-entry hardware firewall.

## Enable it

1. Open **Privacy → USB Devices**.
2. Turn on **New keyboard approval**.
3. Choose the approval window: 30 seconds, 60 seconds, or 5 minutes.
4. Keep a known working mouse available. Administrator rights are required for
   Windows device disable/enable operations.

The setting requests a machine policy in Pro's hardened
`%ProgramData%\WinCommander\usb-guard` store. After initial import, that Pro
record is canonical; stale per-user settings do not overwrite it.

## What happens when a keyboard appears

1. Pro's user-level PnP poll observes new HID interfaces.
2. It groups the verified composite-device functions, disables all of them, and
   reads Windows state back. A late sibling cancels any open challenge and makes
   Pro repeat containment for the whole group.
3. Only a fully contained, unambiguous group receives positive actions. A
   containment or topology failure shows **Retry block** and instructs the user
   to unplug the device; it never presents a reassuring Allow button.
4. The global dialog appears even when the main window was in the tray.

## Decisions

- **Keep blocked** re-discovers the group, disables every live HID sibling, and
  confirms the result. If that fails, unplug the device.
- **Allow once** requires one six-click visual challenge and applies only to the
  currently observed group.
- **Always trust** requires two independently generated six-click challenges.
  It appears only when every contained HID shares one stable hardware serial.
  Serial-less, mixed-identity, or ambiguous composite devices cannot receive
  permanent trust.

Each click is submitted to Pro. Pro—not the WebView—validates the next digit,
returns a freshly shuffled keypad, rotates the challenge after a wrong click,
and locks further challenges for one minute after repeated failures. Old/manual
USB allow-list entries are not treated as having completed this ceremony.

## What the click challenge proves

It is a human-presence speed bump for a blind HID macro. It does **not** prove
that the clicks came from a separate trusted mouse: browser pointer events do
not identify the physical device, and a composite or screen-aware attacker may
be able to drive them.

The current poll is reactive after enumeration. It cannot guarantee:

- blocking the first injected keystroke or first device access;
- detecting a very fast detach/replug that reuses every Windows identifier;
- protection before Windows boots;
- authentication of a Flipper Zero, Rubber Ducky, O.MG cable, or spoofed
  VID/PID/serial;
- protection from mouse-only, network-capable cable, DMA, or Thunderbolt paths.

For managed organisations, use Microsoft's machine-scoped
[Device Installation Restrictions](https://learn.microsoft.com/windows/client-management/client-tools/manage-device-installation-with-group-policy)
or MDM allow/default-deny policy as the stronger pre-install control. Configure
every parent and logical function of a multifunction device and keep tested
recovery input hardware. WinCommander's popup is the reactive decision layer,
not a replacement for that policy.

## Windows Server and RDS

Windows Server 2019/2022/2025 with Desktop Experience is the GUI target; Server
Core is not. Configure USB Guard using the designated managed administrator or
service identity. The hardened ProgramData store intentionally does not grant
all local users write access, so another non-admin RDS session cannot become the
machine-policy writer. Pooled cross-principal administration remains
unsupported until the signed Windows service broker is implemented and tested.

## Acceptance still required

Source checks and unit tests do not replace physical validation. Before claiming
deployment protection, test on a disposable host with normal keyboards, two
identical serial-less HIDs, a composite keyboard+mouse/storage device, Flipper
Zero, Rubber Ducky, and O.MG hardware. Include delayed/jittered scripts,
mouse-only input, containment failure, challenge expiry/lockout, late composite
enumeration, detach/replug, reboot/preboot, two RDS sessions, and entitlement
loss.
