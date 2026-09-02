# Positioning — WinCommander

WinCommander is for people who want a visible, controllable privacy and hardening posture on Windows instead of opaque optimizer software or disconnected scripts.

## Who it is for

- Privacy-conscious Windows power users, security researchers, journalists, and operators.
- People who want a measurable hardening state, not a collection of registry files.
- Licensed Pro customers who need advanced safeguards or self-hosted fleet capability.

It is not for users seeking a hidden “make it faster” button, cross-platform coverage, or an antivirus/EDR replacement. See [NON-GOALS.md](NON-GOALS.md).

## Value proposition

WinCommander turns Windows hardening into a single local console. You define the intended state, the app checks the machine’s current state, and the sovereignty score shows what to improve next. Risky actions are explicit and warnings are part of the product—not an afterthought.

## Principles

- **Auditable Free tier** — Free is public under AGPL-3.0.
- **Clear commercial boundary** — Pro is private and proprietary, with backend entitlement enforcement.
- **Local-first** — no product telemetry or account requirement.
- **Risk-aware controls** — irreversible and security-reducing actions are surfaced before execution.
- **Evidence before claims** — planned cryptography, source scaffolding, or a successful happy path is not marketed as shipped protection.

## Post-quantum positioning

WinCommander is **not quantum-resistant end to end today**. Current releases
still depend on classical public-key trust for product and platform operations.
The accurate current wording is:

> WinCommander uses AES-256 authenticated encryption for protected application
> state and has a documented migration programme for standardized post-quantum
> key establishment and signatures. Current releases are not quantum-resistant
> end to end.

After every first-party claim gate, release-verification gate, migration gate,
and independent-review gate has passed for an identified release, the approved
scoped headline is:

> Quantum-resistant protection for WinCommander-controlled commands, updates,
> licences, and evidence — built on standardized post-quantum cryptography.

That future wording does not extend automatically to Windows, firmware,
Authenticode, TPM, public TLS, Tailscale, VeraCrypt, identity providers,
timestamp authorities, or other external systems. See
[SECURITY.md](SECURITY.md#cryptography-and-post-quantum-status) for the public
claim boundary.

Never use **“quantum-proof,” “unbreakable,” “future-proof,” “quantum key
cryptography,”** or an unscoped **“post-quantum secure.”** WinCommander is
pursuing standardized post-quantum cryptography, not quantum key distribution.

See [OPEN_CORE.md](OPEN_CORE.md) for the licence split and [FEATURES.md](FEATURES.md) for capabilities.
