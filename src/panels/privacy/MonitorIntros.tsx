// src/panels/privacy/MonitorIntros.tsx
//
// "How it works?" dialogs for the four monitoring features (Paste,
// Decoy, Ransomware, Panic-Keyword). Each shows the looping canvas
// animation + a short explainer. Where it makes sense, a small
// interactive demo input lets the user paste / type real text and
// see the matcher fire in-animation. The demo input runs LOCAL
// pattern matching only — nothing crosses an IPC boundary, nothing
// touches the real backend.

import { Dialog, Button, InputGroup, Icon } from "@/components/ui/bp";
import { useRef, useState } from "react";
import {
    PasteMonitorAnimation,
    DecoyMonitorAnimation,
    RansomwareMonitorAnimation,
    PanicKeywordAnimation,
    MonitorAnimationHandle,
} from "./MonitorAnimations";

interface IntroProps {
    isOpen: boolean;
    onClose: () => void;
}

// Pattern-matching subset mirroring the actual clipboard-guard patterns
// (used only for the in-dialog interactive demo, so the user sees the
// real engine's behaviour — minus the toast + panic firing).
const DEMO_PASTE_PATTERNS: Array<{ name: string; re: RegExp }> = [
    { name: 'AWS_KEY', re: /\b(?:AKIA|ASIA)[A-Z0-9]{16}\b/ },
    { name: 'JWT', re: /\beyJ[A-Za-z0-9_-]+\.eyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\b/ },
    { name: 'BIP-39', re: /\b(?:[a-z]+\s+){11,23}[a-z]+\b/i },
    { name: 'Private Key', re: /-----BEGIN [A-Z ]*PRIVATE KEY-----/ },
];

function IntroFrame({ title, subtitle, body, children, footer, onClose }: {
    title: string;
    subtitle: string;
    body: string;
    children: React.ReactNode;
    footer?: React.ReactNode;
    onClose: () => void;
}) {
    return (
        <Dialog
            isOpen
            onClose={onClose}
            style={{
                width: 620,
                background: 'var(--color-bg-secondary)',
                border: '1px solid var(--color-border)',
                borderRadius: '8px',
            }}
            title={title}
        >
            <div className="p-6 flex flex-col items-center gap-5">
                <div
                    className="relative w-full overflow-hidden rounded-md border border-[var(--color-border)] flex justify-center items-center"
                    style={{ height: 340, background: 'var(--color-bg-tertiary)' }}
                >
                    {children}
                </div>
                <div className="text-center max-w-[500px]">
                    <h3 className="text-sm font-semibold text-[var(--color-accent)] mb-2">
                        {subtitle}
                    </h3>
                    <p className="text-xs text-[var(--color-text-muted)] leading-relaxed">
                        {body}
                    </p>
                </div>
                {footer}
                <Button text="Got it" className="w-full mt-1 intro-btn-solid" onClick={onClose} />
            </div>
        </Dialog>
    );
}

// ─── Clipboard Guard ────────────────────────────────────────────
export function PasteMonitorIntro({ isOpen, onClose }: IntroProps) {
    const animRef = useRef<MonitorAnimationHandle>(null);
    const [demoText, setDemoText] = useState("");
    const [demoResult, setDemoResult] = useState<{ kind: 'ok' | 'match'; label: string } | null>(null);
    if (!isOpen) return null;

    const runDemo = (text: string) => {
        setDemoText(text);
        const hit = DEMO_PASTE_PATTERNS.find(p => p.re.test(text));
        if (hit) {
            setDemoResult({ kind: 'match', label: hit.name });
            animRef.current?.fireAlarm(2.5);
        } else if (text.trim()) {
            setDemoResult({ kind: 'ok', label: 'no pattern matched' });
        } else {
            setDemoResult(null);
        }
    };

    return (
        <IntroFrame
            title="What is paste monitoring?"
            subtitle="Watches your clipboard for credentials"
            body="Every clipboard event is checked locally against patterns for AWS keys, JWT tokens, BIP-39 seed phrases, private keys, and other high-value secrets. On match, the clipboard is cleared and the lockdown flow can fire. Nothing crosses an IPC boundary — patterns run in-process."
            onClose={onClose}
            footer={
                <div className="w-full flex flex-col gap-2">
                    <div className="text-[10px] uppercase tracking-wider text-[var(--color-text-muted)] font-mono">
                        Try it — paste or type anything (try an AWS key like AKIAABCDEFGHIJKLMNOP)
                    </div>
                    <InputGroup
                        placeholder="Paste or type sample text here…"
                        value={demoText}
                        onChange={(e) => runDemo(e.currentTarget.value)}
                        rightElement={
                            demoResult ? (
                                <span
                                    className="flex items-center gap-1 px-2 text-[10px] font-mono font-bold"
                                    style={{ color: demoResult.kind === 'match' ? 'var(--color-danger)' : 'var(--color-success)' }}
                                >
                                    {demoResult.kind === 'match' ? <Icon icon="warning-sign" size={11} /> : <Icon icon="tick" size={11} />}
                                    {demoResult.kind === 'match' ? demoResult.label : 'CLEAN'}
                                </span>
                            ) : undefined
                        }
                    />
                </div>
            }
        >
            <PasteMonitorAnimation ref={animRef} />
        </IntroFrame>
    );
}

// ─── File Access Monitor ────────────────────────────────────────
export function DecoyMonitorIntro({ isOpen, onClose }: IntroProps) {
    if (!isOpen) return null;
    return (
        <IntroFrame
            title="What is file access monitoring?"
            subtitle="Tripwire files that alert when touched"
            body="Drop decoy files — plausible account notes, wallet checklists, and client lists — in places an intruder is likely to inspect. Pro watches those exact paths. A real content change, rename, or removal raises an alert; optional Windows auditing can also report opens. Background metadata is ignored, and no lockdown action is implied."
            onClose={onClose}
        >
            <DecoyMonitorAnimation />
        </IntroFrame>
    );
}

// ─── Mass-Encryption Alarm ──────────────────────────────────────
export function RansomwareMonitorIntro({ isOpen, onClose }: IntroProps) {
    if (!isOpen) return null;
    return (
        <IntroFrame
            title="What is ransomware monitoring?"
            subtitle="Catches mass-encryption attacks early"
            body="Ransomware often modifies many files quickly. The Free monitor raises a loud alarm when modifications cross the configured rolling threshold (default: 50 files in 30 seconds). Pro can then attribute a sufficiently evidenced process and optionally suspend or terminate it. Build tools and bulk file operations can also trigger the pattern, so tune the threshold and review the evidence."
            onClose={onClose}
        >
            <RansomwareMonitorAnimation />
        </IntroFrame>
    );
}

// ─── Remote Access Monitor ──────────────────────────────────────
export function RemoteAccessMonitorIntro({ isOpen, onClose }: IntroProps) {
    if (!isOpen) return null;
    return (
        <IntroFrame
            title="What is remote access monitoring?"
            subtitle="Alerts the moment someone may be controlling your PC"
            body="Watches for an active incoming remote-control session — AnyDesk, TeamViewer, RustDesk, VNC, RDP, Chrome Remote Desktop or Windows Quick Assist. A tool merely being installed and open stays quiet; but when there's an established connection on its port (or a fresh incoming line in its own log), you get a red alert. It can't tell an invited IT session from an attacker — only that a session is happening — so the alert asks: if you didn't start this, end the session and disconnect now."
            onClose={onClose}
        >
            <DecoyMonitorAnimation />
        </IntroFrame>
    );
}

// ─── Panic Keyword (Coercion phrase) ───────────────────────────
export function PanicKeywordIntro({ isOpen, onClose }: IntroProps) {
    const animRef = useRef<MonitorAnimationHandle>(null);
    const [demoText, setDemoText] = useState("");
    const [demoResult, setDemoResult] = useState<{ kind: 'ok' | 'match' } | null>(null);
    if (!isOpen) return null;

    // Local-only demo phrase. The real engine compares SHA-256 hashes
    // of registered phrases against keystroke buffers; we approximate
    // that for the in-dialog demo by case-insensitive substring match
    // on a fixed sample phrase.
    const DEMO_PHRASE = 'red dragon nine';

    const runDemo = (text: string) => {
        setDemoText(text);
        if (text.toLowerCase().includes(DEMO_PHRASE)) {
            setDemoResult({ kind: 'match' });
            animRef.current?.fireAlarm(2.5);
        } else if (text.trim()) {
            setDemoResult({ kind: 'ok' });
        } else {
            setDemoResult(null);
        }
    };

    return (
        <IntroFrame
            title="What are Lockdown Words?"
            subtitle="A phrase you type — anywhere — to trigger lockdown discreetly"
            body="Register a short phrase (something you'd never type by accident). The system intercepts keystrokes everywhere — chat, browser, terminal, anywhere — and SHA-256 hashes each token. If a hash matches a registered phrase, lockdown triggers. Useful under pressure: type something innocuous-looking with your lockdown word inside it and the lockdown triggers without anyone seeing why."
            onClose={onClose}
            footer={
                <div className="w-full flex flex-col gap-2">
                    <div className="text-[10px] uppercase tracking-wider text-[var(--color-text-muted)] font-mono">
                        Demo phrase: <span className="text-[var(--color-accent)]">red dragon nine</span> — type it inside any sentence
                    </div>
                    <InputGroup
                        placeholder="e.g. 'okay just give me a sec, red dragon nine'"
                        value={demoText}
                        onChange={(e) => runDemo(e.currentTarget.value)}
                        rightElement={
                            demoResult ? (
                                <span
                                    className="flex items-center gap-1 px-2 text-[10px] font-mono font-bold"
                                    style={{ color: demoResult.kind === 'match' ? 'var(--color-danger)' : 'var(--color-success)' }}
                                >
                                    {demoResult.kind === 'match' ? <Icon icon="warning-sign" size={11} /> : <Icon icon="tick" size={11} />}
                                    {demoResult.kind === 'match' ? 'LOCKDOWN FIRED' : 'NO MATCH'}
                                </span>
                            ) : undefined
                        }
                    />
                </div>
            }
        >
            <PanicKeywordAnimation ref={animRef} />
        </IntroFrame>
    );
}
