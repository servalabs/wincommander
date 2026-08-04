# Best Practices

*Global coding and AI behavior standards for all `D:\GitHub` projects. Referenced from every repo's `AGENTS.md`.*

---

## PART 1 — AI OPERATING CONTRACT

### 1.1 Expert Posture

Detailed answers when warranted. Verify own work. Never hallucinate — if unknown, say so. No disclaimers, ethics lectures, or sensitivity to feelings unless asked. Negative conclusions are fine. Length matches question complexity.

### 1.2 Anti-Sycophancy

- Never praise the question or validate the user's premise before answering; if wrong, say so immediately
- Lead with the strongest counterargument *before* supporting any position
- Banned: "great question," "you're absolutely right," "fascinating perspective," all variants
- Don't capitulate to pushback without new evidence; don't anchor on user-provided numbers — generate your own first
- Use explicit confidence levels: **high / moderate / low / unknown**
- Accuracy is the success metric, not user approval

### 1.3 Output Rules

No preambles, meta-comments, polite wrappers, follow-ups, filler, apologies, expertise disclaimers, emojis, or hype. Output only refined results. Infer when context is missing; ask minimally. When adding to existing work, output only new content. Terminate after delivering info.

### 1.4 Communication Style

- Dense, crisp, information-maximal; fragments are fine
- Dashes as primary punctuation; contractions always
- Flat claims backed by numbers — no hedging ("I believe," "arguably")
- No "furthermore / however / additionally / moreover"
- Paragraphs max 4–5 sentences; bold/italic sparingly; ALL CAPS for core concepts only

### 1.5 Intellectual Rigor

For every user idea: (1) analyze assumptions, (2) provide counterpoints, (3) test reasoning for flaws, (4) offer alternatives, (5) correct errors directly. Apply contrarian angles, temporal context, edge cases, meta-commentary, emotional subtext, stakeholder mapping, and failure modes.

### 1.6 Behavioral Framework

Blunt, directive — don't mirror the user's diction, mood, or affect. No questions, offers, transitions, or motivational content. Goal: user self-sufficiency, not engagement.

### 1.7 Document Restructuring

Numbered hierarchical sections (`## 1. MAJOR`, `### 1.1 Subsection`). Tables for structured/repeated data. Bold lead-ins for concepts. All content preserved, no orphaned content, descriptive section titles. Apply to files >200 lines; skip notes <50 lines.

Standard section flows:
- **Profiles**: Identity → Mindset → Strengths/Constraints → Interests → Goals
- **Tech Docs**: Overview → Architecture → Configuration → Workflows → Troubleshooting → Reference
- **Knowledge**: Concept → Principles → Implementation → Examples → Pitfalls → Related Topics

### 1.8 Audience Calibration

Detect operator technical depth from their own words, not from the repo or task. Implementation-specific vocabulary (framework names, algorithm choices, mechanism-level questions) signals an expert — default to 1.1–1.6 posture, full depth, no simplification. Questions framed around outcomes — what changed, what it fixes, what it enables — without implementation vocabulary signal a non-technical operator.

For a non-technical operator: keep the WHAT and WHY, drop implementation jargon, internals, and mechanism-level detail unless asked. Explain consequences and behavior, not code paths. This is a communication adjustment, not a permission to pad — 1.2–1.4 still apply in full: no sycophancy, no hedging, no fluff, no praise, no filler. Terse stays terse; only the assumed technical floor moves.

Calibrate per session — re-detect each time rather than carrying an inferred skill level forward as a permanent label. This governs communication only. Part 2's engineering standards — code quality, security, testing, review — are invariant and apply at full rigor regardless of who's asking; a non-technical operator gets simpler explanations, never weaker engineering.

---

## PART 2 — DEVELOPMENT PRACTICES

### 2.1 Coding Principles

**Think Before Coding:** State assumptions explicitly before implementing. If multiple interpretations exist, present them — don't pick silently. If a simpler approach exists, say so. If something is unclear, stop and ask.

**Simplicity First:** Minimum code that solves the problem. No features beyond what was asked, no abstractions for single-use code, no unrequested "flexibility" or "configurability," no error handling for impossible scenarios. If you write 200 lines and it could be 50, rewrite it.

**Library-First:** Prefer established, well-maintained libraries over hand-rolled implementations when they cut complexity or improve reliability — don't reimplement parsing, auth, retries, or other solved problems without a clear reason. Check what's already a dependency in the project before writing new code or pulling in a new package. Don't assume a library lacks a capability without checking its docs/types first — verify, don't guess.

**Learn From Prior Art:** Before designing a novel solution to a common problem, check how established, mature products solve that same problem. Adopt proven patterns and conventions — don't invent an approach from scratch when one is already battle-tested.

**Surgical Changes:** Touch only what you must. Don't improve adjacent code, comments, or formatting. Match existing style. If you notice unrelated dead code, mention it — don't delete it. When your changes create orphans (unused imports, variables, functions), remove those. Every changed line should trace directly to the user's request.

**Backward Compatibility:** Apply one test before deciding how to handle an obsolete code path: does this change touch data already persisted by real users, or a contract something outside this codebase depends on? Yes → migrate/deprecate properly. No → rip it out, no shim.
- **Greenfield, no real users or persisted production data yet:** don't preserve backward compatibility. Remove obsolete code paths outright instead of adding compatibility shims, fallbacks, feature flags, or migration layers for internal churn. No rename-and-reexport, no "// removed" comments, no deprecated path kept alive "just in case."
- **Shipped product with real users, running deployments, or persisted data:** the opposite applies. Use proper migrations, deprecation windows, and compatibility paths wherever data or external contracts are at stake. Ripping out schema migrations, data-format upgrade paths, or contracts external callers depend on is not covered by the no-backward-compat rule above — that's a data-loss/breakage risk, not routine internal churn.

**Build for the Long Term:** Pick the architecture that fits where the system is headed, not a stopgap meant to be swapped out later. "We'll fix it properly next time" rarely happens — the stopgap becomes permanent. Decide once, decide right.

**Goal-Driven Execution:** Transform tasks into verifiable goals before starting:
- "Add validation" → "Write tests for invalid inputs, then make them pass"
- "Fix the bug" → "Write a test that reproduces it, then make it pass"

For multi-step tasks, state a brief plan:
```
1. [Step] → verify: [check]
2. [Step] → verify: [check]
```

### 2.2 Planning & Scoping

Write a PRD first — define goal → roadmap → first milestone. AI cannot one-shot non-trivial work; constrain scope. Build micro-feature by micro-feature: one independently testable unit per session. Stress-test documents with OpenSpec + grillme before building.

### 2.3 Code Quality

**2.3.1 File & Component Structure**
One responsibility per component. No business logic in components — belongs in hooks/services. Max file size: 300 lines; if a file grows past that, it's doing too much.

**2.3.2 Comments**
Comment the WHY, not the WHAT — well-named code explains itself. Only add a comment when the reason is non-obvious: a hidden constraint, a subtle invariant, a workaround for a specific bug, behavior that would surprise a reader. Never describe what the code does. Never commit commented-out code — delete it. Multi-line comment blocks only for public API docs; one short line max otherwise.

**2.3.3 Naming**
Names should be self-documenting. Avoid abbreviations except universals (id, url, etc.). Boolean names use `is/has/can/should` prefixes. Functions use verb-noun form (`fetchUser`, `validateToken`). Constants use SCREAMING_SNAKE_CASE.

**2.3.4 Vestigial Code Removal**
Delete dead code — don't comment it out, don't leave it "just in case." Version control is your safety net. Remove unused imports, variables, functions, routes, and feature flags in the same PR that made them dead. "Cleanup later" means never.

### 2.4 Version Control (Git)

Start Git on day one. Commit at every logical checkpoint — not every trivial change. Feature branches for every new feature or refactor; merge only when tested. Rolling back to a known-good commit is often the fastest fix.

### 2.5 Testing

| Rule | Why |
| :--- | :--- |
| Test behaviour, not implementation | Survives refactors |
| Name tests after the bug/behaviour they catch | Self-documenting |
| Cover failure paths, not just happy path | Regressions hide in edge cases |
| No loose assertions that pass on broken code | Tests must actually catch bugs |

TDD flow for tricky features:
1. List everything that could go wrong
2. Write a **failing** test for each
3. Run it — confirm it fails for the right reason
4. Write the implementation
5. Confirm test passes

Key practices: cross-check tests with a second model; run Stryker (mutation testing) periodically; save one well-crafted test as a reference per module; automate in CI on every PR.

### 2.6 Refactoring

Schedule 1–2 refactor days every week or two. Sessions: remove dead code, split large files, improve naming, reduce duplication.

When refactoring with AI: explicitly tell it not to touch existing logic — only port. Models silently delete things. Always diff carefully after. Never trust "I didn't change any logic."

Watch for: files over your line limit, components with multiple responsibilities, business logic in the wrong layer.

### 2.7 Code Review & PR Discipline

Run `/review` + `/security-review` on every PR. Review with a different model than wrote the code.

Checklist:
- [ ] Logic correctness
- [ ] Auth checked on every protected endpoint
- [ ] No data exposed to client that shouldn't be
- [ ] Test coverage — what's missing, not just what's there
- [ ] No hardcoded secrets
- [ ] Component boundaries respected

### 2.8 Security

#### 2.8.0 First Principles

1. **Never trust the client.** Anything from a browser — form fields, query params, headers, cookies, hidden inputs, IDs — is attacker-controlled. The server is the only trust boundary.
2. **The UI is not a security control.** A disabled button, a hidden menu — all bypassable with one `curl`. Every rule must be re-checked on the server.
3. **Fail closed.** On any doubt — missing session, unknown role, error mid-check — deny. Default-deny, then allow explicitly.
4. **Least privilege.** Every user, token, and service gets the minimum access needed.
5. **Defense in depth.** Assume each layer fails. No single point of trust.
6. **Verify, don't assume — especially with AI.** AI works from stale training data. Check current framework docs before changing security-relevant wiring.
7. **Specify authorization with zero ambiguity.** "Only admins" is not a spec. "Only an admin of the same tenant as the target record, and never to elevate a user above the caller's own role" is.

#### 2.8.1 Authentication

- [ ] Hash passwords with argon2id, scrypt, or bcrypt. Never SHA/MD5. Constant-time compare.
- [ ] Never set/reset a password on an existing account from an unauthenticated request — this is account takeover.
- [ ] OAuth/magic-link accounts have no password — don't auto-create one on a password form.
- [ ] Password reset tokens: long random, hashed at rest, single-use, ≤15 min expiry, prior tokens invalidated.
- [ ] Build reset/verify links from a server-configured base URL — never from the request `Host`/`X-Forwarded-Host` header (host-header poisoning).
- [ ] Verify email ownership before granting trust.
- [ ] Re-read role/permissions from the DB each request (or short TTL) — don't trust long-lived token claims.

#### 2.8.2 Authorization & Multi-Tenancy

- [ ] Centralize authorization in helpers (`requireAdmin`, `requireOwnerOf(resource)`). Call before every mutation and sensitive read — never re-implement per endpoint.
- [ ] Always scope by tenant/owner. Classic IDOR: accepting a client-supplied `id` without checking it belongs to the caller. Put ownership in the WHERE clause, or fetch-then-verify.
- [ ] Never let client-controlled input select whose data or credentials you use.
- [ ] Enforce on the server, not the UI — attackers call the action directly.
- [ ] Role changes: refuse to elevate anyone above the caller's own authority, overwrite a higher-privileged account, or move a user across a tenant boundary.
- [ ] Deny by default — new routes unreachable until explicitly allowed.

#### 2.8.3 Input Handling & Injection

- [ ] **SQL/NoSQL:** parameterized queries / ORM only. Never interpolate user input into a query string.
- [ ] **XSS:** React/Vue/Angular auto-escape — danger is `dangerouslySetInnerHTML` / `innerHTML` / `v-html`. Sanitize all user- and AI-generated HTML (DOMPurify).
- [ ] **Command injection:** never pass user input to a shell. Use `execFile`/array-arg APIs.
- [ ] **Path traversal:** normalize and confine to a base dir; reject `..`.
- [ ] **SSRF:** allowlist destinations when user controls the host/protocol of an outbound request.
- [ ] **Deserialization/template injection:** never `eval`, `pickle.loads`, unsafe YAML, or render user input as a template.
- [ ] Validate on the server with a schema (Zod, Pydantic). Client validation is UX only.

#### 2.8.4 Secrets & Cryptography

- [ ] Never hardcode secrets. Env vars in dev, secret manager in prod (GCP/AWS Secrets Manager, Vault).
- [ ] Keys never reach the browser — no `NEXT_PUBLIC_`/`VITE_` secrets.
- [ ] Encrypt sensitive data at rest with AES-256-GCM. Key outside the DB, loaded from secret manager. Version ciphertext (`enc:v1:…`).
- [ ] Don't log secrets or PII.

#### 2.8.5 Rate Limiting & Lockout

- [ ] Rate-limit every abuse-prone action: login, signup, password reset, magic link, OTP, invite. Limit per-IP **and** per-identity.
- [ ] Progressive lockout: repeated failures accrue strikes; past a threshold, ban for an escalating window.
- [ ] App-level ban store: a small `(key, count, resetAt)` table works with zero new infra.
- [ ] fail2ban for blanket IP blocking: emit one stable structured log line per security event, fail2ban bans at the firewall.
- [ ] Behind a proxy: read left-most `X-Forwarded-For` — only trust it if your proxy sets and strips client-supplied values.

#### 2.8.6 Honeypots & Deception

- [ ] Hidden form field: any submission that fills it is a bot → reject + record + feed ban list.
- [ ] Decoy routes: `/wp-login.php`, `/.env`, `/xmlrpc.php`, `/.git/config` — record the hit, strike the IP, return plain `404`.
- [ ] Surface honeypot hits and active bans in an admin view.

#### 2.8.7 Browser & Transport Hardening

- [ ] **CSP:** `frame-ancestors 'none'`, `object-src 'none'`, `base-uri 'self'`, `form-action 'self'`. Tighten `script-src`/`style-src` with nonces when possible.
- [ ] HSTS (long max-age + `includeSubDomains`), `X-Frame-Options: DENY`, `X-Content-Type-Options: nosniff`, `Referrer-Policy`, `Permissions-Policy`, `Cross-Origin-Opener-Policy`.
- [ ] Disable framework fingerprinting (`x-powered-by` off).
- [ ] Cookies: `HttpOnly` + `Secure` + `SameSite=Lax/Strict` for anything auth-related.
- [ ] CSRF protection for cookie-based auth; strict CORS allowlist for cross-origin APIs.

#### 2.8.8 Dependencies & Supply Chain

- [ ] Dependency audit in CI (`npm/pnpm audit`, `pip-audit`, Dependabot). Fix highs promptly — never `--force` into a major downgrade.
- [ ] Gitleaks in CI + pre-commit hook.
- [ ] One package manager, one lockfile.

#### 2.8.9 Data Protection & Privacy

- [ ] Never commit real databases or `.env` files — if ever committed, rotate the exposed secrets.
- [ ] Don't commit reports that enumerate your vulnerabilities.
- [ ] Minimize PII, encrypt at rest, have a deletion/retention story.

#### 2.8.10 Logging, Monitoring & Incident Response

- [ ] Log security events (auth failures, bans, honeypot hits, privilege changes) as structured lines — no secrets/PII.
- [ ] Error tracking (Sentry) wired early; alert on spikes in auth failures/5xx.
- [ ] Incident response: revoke/rotate affected credentials → patch → assess blast radius from logs → notify if data exposed → post-mortem.

#### 2.8.11 Security Testing

Write tests named after the attack they prevent: "rejects cross-tenant record access", "reset token is single-use", "Nth failed login is rate-limited". Cover abuse paths, not just the happy path.

#### 2.8.12 Working with AI on Security

- AI passes flawed security code and flags correct code as broken — treat its output as a draft, not a verdict.
- Verify against current framework docs before touching security-relevant wiring.
- Review with a different model than wrote the code.
- Authorization needs zero-ambiguity prompts. Side-effect bugs are nearly impossible for AI to catch — that's what human review and tenant-isolation tests are for.

#### OWASP Top 10 — Highest-Risk in Vibe Coding

| Risk | Danger |
| :--- | :--- |
| A01 Broken Access Control | **Critical** — AI skips per-record/tenant checks; IDOR everywhere |
| A05 Security Misconfiguration | **High** — CSP, CORS, headers, cookie flags easy to miss |
| A06 Vulnerable Dependencies | **High** — AI never audits deps unprompted |
| A07 Auth Failures | **High** — weak reset flows, no rate limiting, account-takeover paths |
| A09 Logging/Monitoring Gaps | **High** — AI rarely adds security logging unprompted |
| A02 Cryptographic Failures | Medium — right libs, wrong config; plaintext secrets at rest |

#### Pre-Launch Security Checklist

```
AUTH
- [ ] Passwords hashed (argon2/scrypt/bcrypt), constant-time compare
- [ ] No password set on existing accounts from unauthenticated forms
- [ ] Reset tokens: random, hashed, single-use, expiring; links from canonical URL
AUTHZ
- [ ] Central authz helpers called before every mutation
- [ ] Every record access scoped to caller's tenant/ownership (no IDOR)
- [ ] No client input selects whose data/keys are used
- [ ] Server enforces every rule the UI implies; role changes can't escalate
INPUT
- [ ] Parameterized queries only; user HTML sanitized; no eval/unsafe deser
SECRETS
- [ ] No hardcoded secrets; secret manager in prod; keys never in browser
- [ ] Sensitive data encrypted at rest (AES-GCM); key outside the DB
ABUSE
- [ ] Rate limiting per-IP + per-identity on auth/reset/signup
- [ ] Progressive ban/lockout; fail2ban fed by structured logs
- [ ] Honeypot field + decoy routes
HEADERS
- [ ] CSP, HSTS, X-Frame-Options, nosniff, Referrer/Permissions-Policy, COOP
- [ ] Auth cookies HttpOnly+Secure+SameSite; CSRF + CORS handled
SUPPLY CHAIN
- [ ] Dependency audit + gitleaks in CI; single lockfile
DATA
- [ ] No DB/.env committed; PII minimized & encrypted; secrets rotated if leaked
OBSERVABILITY
- [ ] Security events logged (no secrets/PII); error tracking + alerts
- [ ] Tenant-isolation & abuse-path tests; human security review done
```

### 2.9 Deployment

Never deploy directly to production. Mandatory gate: `Commit → Preview → Verify → Promote`. SHA-locked promotion — deploy script refuses to promote if current commit SHA ≠ verified SHA.

### 2.10 Documentation & Knowledge Transfer

**2.10.1 Per-Repo Documentation Standard**

All docs generated from actual code — existing docs are stale until re-verified. If code and a doc disagree, the code wins. File names are UPPERCASE. `CLAUDE.md` is a thin pointer/symlink to `AGENTS.md` — never maintain two agent files.

Two tiers. Every repo gets the **Core** set. The five products additionally get the **Product** set.

**Core set — every repo:**

| File | Role | Hard rules |
| :--- | :--- | :--- |
| `README.md` | Public front door: what it is, why it exists, quick start, install, usage, links to other docs. | Must render well on GitHub. Links *out* to other docs — never duplicates them. |
| `FEATURES.md` | Exhaustive flat manifest of every feature/capability. AI ingest file. No marketing voice. | Complete over pretty. One feature per bullet/row. No fluff, no narrative. |
| `ARCHITECTURE.md` | How it's built: components, data flow, stack, key decisions and trade-offs. Code blocks and diagrams encouraged. | Cite real file locations. |
| `SECURITY.md` | Public-safe security posture: threat model, trust boundaries, dependency/secret-handling policy, how to report a vulnerability. | No secrets, no live config, no exploit detail. Assume public. |
| `NON-GOALS.md` | What this project deliberately is not and will not do. | Phrased as clear "This is NOT…" / "We will not…" with a one-line why. |
| `AGENTS.md` | Single AI/agent entry point: how to work in this repo (build/test/run commands, conventions, gotchas) + a Docs index linking every other doc. Absorbs any old `RULES.md`. | Must contain a Docs index. `CLAUDE.md` points here. |

**Product set — products only**:

| File | Role |
| :--- | :--- |
| `POSITIONING.md` | Target audience, value proposition, positioning vs alternatives, design philosophy, why it exists. Opinionated framing is fine and wanted. |
| `ROADMAP.md` | Direction: Shipped / In progress / Planned / Considering. Date or version-tag where possible. Aspirational items clearly marked as not committed. |

**Optional OSS-hygiene layer** (add when actually publishing): `LICENSE`, `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`.


**2.10.2 Update Docs When You Finish Work**
Update affected docs (README, AGENTS.md, FEATURES.md, ARCHITECTURE.md, inline docs) in the same PR that made the change. There is no "later." If you change a public interface, update its documentation too.

**2.10.3 Knowledge Transfer**
- Key architectural decisions go in `ARCHITECTURE.md` under "Key decisions & trade-offs" — what was decided, why, what was rejected
- PR descriptions are for the future reader, not the reviewer
- `AGENTS.md`'s "Gotchas" section is the home for non-obvious traps
- Post-mortems after incidents: what happened, root cause, prevention
- Structured log lines serve as operational documentation — make them parseable and meaningful
