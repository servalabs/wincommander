# Unknown-keyboard approval — public limits

WinCommander Pro can react to a newly observed keyboard by holding it for an
operator decision. This reduces the time an unknown Human Interface Device is
trusted after Windows detects it; it is not a hardware firewall.

## What it can do

- Notice a newly attached keyboard after Windows reports it.
- Attempt to contain the related device group.
- Ask an authorized operator to allow it temporarily, trust it, or keep it
  blocked.
- Fail closed when the device group or containment result is ambiguous.

## What it cannot prove

- It cannot guarantee that the first keystroke was blocked.
- It does not operate before Windows boots.
- A click challenge adds human-presence friction; it does not prove the click
  came from a separate trusted physical mouse.
- Polling cannot guarantee containment of every very fast detach/replug or
  identity-reuse sequence.
- A non-admin RDS user may not be able to administer machine-wide policy.
- It does not replace firmware controls, Device Installation Restrictions,
  WDAC/App Control, MDM/GPO, port control, or physical security.

## Recommended layered setup

Use Pro approval for the reactive decision point, and use Windows
Device Installation Restrictions or organization policy when unknown keyboards
must be blocked before normal driver installation. Test the policy with the
actual hardware, Windows edition, Server/RDS setup, docks/hubs, and recovery
process used in production.

Report a containment bypass or unsafe enablement result privately through
[SECURITY.md](../SECURITY.md).
