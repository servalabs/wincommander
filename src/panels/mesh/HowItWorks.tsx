/**
 * HowItWorks.tsx  –  Private Mesh VPN explainer popup
 *
 * • Full-screen modal with dark overlay
 * • 5-slide story: problem → danger → solution → protection → everywhere
 * • Auto-opens on first visit; can be manually triggered after that
 * • Fully dark + light mode via CSS custom properties
 */

import { useEffect, useRef, useState, useCallback } from "react";
import { Icon } from "@/components/ui/bp";
import './HowItWorks.css';

// ─── Light-mode colour map ────────────────────────────────────────────────────
// Bright neon colours (cyan, etc.) are invisible on white backgrounds.
// These alternatives are readable on both white and light-grey surfaces.
const LIGHT_COLORS: Record<string, string> = {
    "#ef4444": "#dc2626",   // red    → slightly deeper
    "#f97316": "#c2410c",   // orange → burned orange
    "#00f2ff": "#0d9488",   // neon cyan → teal-600 (NOT blue, clear on white)
    "#a78bfa": "#6d28d9",   // violet → deeper purple
    "#34d399": "#059669",   // emerald → deeper green
};

// Walk up the DOM tree from the modal element to find bp5-light/dark on ANY ancestor.
// This is more reliable than only checking document.body.


// ─── Slide data ───────────────────────────────────────────────────────────────

type SlideId = "problem" | "solution" | "security";

interface Slide {
    id: SlideId;
    chip: string;
    chipColor: string;
    title: string;
    subtitle: string;
    body: string;
}

const SLIDES: Slide[] = [
    {
        id: "problem",
        chip: "THE PROBLEM",
        chipColor: "#ef4444",
        title: "Your devices are not\nalways together",
        subtitle: "A laptop at home and a PC at work cannot safely talk to each other by default.",
        body: "• Your devices may be in different places.\n• Opening them to the public internet is risky.\n• Router and firewall settings are confusing.\n• Private Mesh makes them feel like they are on the same safe private network.",
    },
    {
        id: "solution",
        chip: "THE SOLUTION",
        chipColor: "#00f2ff",
        title: "Private Mesh: secure\nIPs with full internet",
        subtitle: "Your home and office devices in one encrypted network — anywhere",
        body: "Private Mesh assigns every device a secure private IP (100.x.x.x) that works across the internet — no public IP ever exposed. Your home laptop, office desktop, and mobile phone stay connected in one encrypted network wherever you are.",
    },
    {
        id: "security",
        chip: "UNDER THE HOOD",
        chipColor: "#a78bfa",
        title: "Military-grade encryption\nfor every connection",
        subtitle: "WireGuard® with ChaCha20-Poly1305 — the same cipher used in modern TLS.",
        body: "Every packet between devices is encrypted end-to-end using the Noise IK protocol over X25519 key exchange. There is no central server that can read your traffic. Keys are derived per-session so past traffic stays safe even if a device key leaks.",
    },
];

const SLIDE_DURATION = 5500;
const FADE_MS        = 320;

// ─── Main component ───────────────────────────────────────────────────────────

interface HowItWorksProps {
    open: boolean;
    onClose: () => void;
}

export default function HowItWorks({ open, onClose }: HowItWorksProps) {
    const [idx, setIdx]         = useState(0);
    const [visible, setVisible] = useState(true);
    const [paused, setPaused]   = useState(false);

    const modalRef              = useRef<HTMLDivElement>(null);


    // Reset to first slide whenever the modal opens
    useEffect(() => {
        if (open) { setIdx(0); setVisible(true); setPaused(false); }
    }, [open]);

    const goTo = useCallback((next: number) => {
        setVisible(false);
        setTimeout(() => { setIdx(next); setVisible(true); }, FADE_MS);
    }, []);

    const prev = () => goTo((idx - 1 + SLIDES.length) % SLIDES.length);
    const next = () => goTo((idx + 1) % SLIDES.length);

    // Close on Escape
    useEffect(() => {
        if (!open) return;
        const handler = (e: KeyboardEvent) => { if (e.key === "Escape") onClose(); };
        window.addEventListener("keydown", handler);
        return () => window.removeEventListener("keydown", handler);
    }, [open, onClose]);

    // ── Direct accent colour injection ─────────────────────────────────────────
    // CSS variable chaining (var(--hiw-accent-lt)) is unreliable in some WebViews.
    // Instead, read the COMPUTED background-color of the modal element (CSS already
    // sets it to #f9fafb in light mode — this is what's actually working), use that
    // to decide the mode, then set --hiw-accent directly on the element.
    // This runs after every render so it's always in sync with the current slide.
    useEffect(() => {
        const el = modalRef.current;
        if (!el) return;

        const applyAccent = () => {
            const bg   = window.getComputedStyle(el).backgroundColor;
            const nums = bg.match(/\d+/g);
            const light = nums
                ? (parseInt(nums[0]) + parseInt(nums[1]) + parseInt(nums[2])) > 382
                : false;
            const slideColor = SLIDES[idx].chipColor;
            const accent     = light ? (LIGHT_COLORS[slideColor] ?? slideColor) : slideColor;
            el.style.setProperty("--hiw-accent", accent);
        };

        applyAccent(); // synchronous — runs after CSS paint

        // Also react to theme switches
        const obs = new MutationObserver(applyAccent);
        obs.observe(document.body,            { attributes: true, attributeFilter: ["class", "data-theme"] });
        obs.observe(document.documentElement, { attributes: true, attributeFilter: ["class", "data-theme"] });
        return () => obs.disconnect();
    }, [idx, open]);

    if (!open) return null;

    const slide = SLIDES[idx];

    return (
        <div className="hiw-overlay" onClick={e => { if (e.target === e.currentTarget) onClose(); }}>
            <div
                ref={modalRef}
                className="hiw-modal"
                onMouseEnter={() => setPaused(true)}
                onMouseLeave={() => setPaused(false)}
            >
                {/* ── Close ── */}
                <button className="hiw-x" onClick={onClose} aria-label="Close">
                    <Icon icon="cross" size={14} />
                </button>

                {/* ── Top strip: slide counter + dots ── */}
                <div className="hiw-topbar">
                    <span className="hiw-counter">
                        {idx + 1} <span className="hiw-counter-of">/ {SLIDES.length}</span>
                    </span>
                    <div className="hiw-dots">
                        {SLIDES.map((s, i) => (
                            <button
                                key={s.id}
                                className={`hiw-dot ${i === idx ? "active" : ""}`}
                                style={i === idx ? {
                                    background: "var(--hiw-accent)",
                                    boxShadow: "0 0 10px var(--hiw-accent)",
                                } : {}}
                                onClick={() => { setPaused(true); goTo(i); }}
                                aria-label={`Slide ${i + 1}`}
                            />
                        ))}
                    </div>
                </div>

                {/* ── Main body: visual + text ── */}
                <div
                    className="hiw-content"
                    style={{ opacity: visible ? 1 : 0, transition: `opacity ${FADE_MS}ms ease` }}
                >
                    {/* Visual — SVG components receive "var(--hiw-accent)" so CSS drives colour */}
                    <div className="hiw-visual-wrap">
                        <SlideVisual id={slide.id} chipColor="var(--hiw-accent)" />
                    </div>

                    {/* Text — chip colour comes entirely from CSS (.hiw-chip uses var(--hiw-accent)) */}
                    <div className="hiw-text-wrap">
                        <span className="hiw-chip">{slide.chip}</span>
                        <h2 className="hiw-title">{slide.title}</h2>
                        <p className="hiw-subtitle">{slide.subtitle}</p>
                        <p className="hiw-body">{slide.body}</p>
                    </div>
                </div>

                {/* ── Navigation ── */}
                <div className="hiw-nav">
                    <button className="hiw-nav-btn" onClick={prev} disabled={idx === 0}>
                        <Icon icon="arrow-left" size={14} /> PREV
                    </button>

                    {idx < SLIDES.length - 1 ? (
                        <button className="hiw-nav-btn hiw-nav-primary" onClick={next}>
                            NEXT <Icon icon="arrow-right" size={14} />
                        </button>
                    ) : (
                        <button className="hiw-nav-btn hiw-nav-primary" onClick={onClose}>
                            GOT IT <Icon icon="tick" size={14} />
                        </button>
                    )}
                </div>

                {/* ── Progress bar ── */}
                <AutoProgress
                    key={idx}
                    duration={SLIDE_DURATION}
                    paused={paused}
                    onDone={() => { if (!paused) idx < SLIDES.length - 1 ? next() : onClose(); }}
                />

            </div>
        </div>
    );
}

// ─── Auto-progress bar ────────────────────────────────────────────────────────
// Properly accumulates elapsed time across pause/resume cycles.
// No CSS transition on the fill — rAF at 60 fps is already smooth.

function AutoProgress({ duration, paused, onDone }: {
    duration: number; paused: boolean; onDone: () => void;
}) {
    const [pct, setPct]      = useState(0);
    const elapsedRef         = useRef(0);            // ms accrued while running
    const segStartRef        = useRef<number | null>(null); // start of current run segment
    const rafRef             = useRef<number | null>(null);
    const doneRef            = useRef(false);
    const onDoneRef          = useRef(onDone);
    onDoneRef.current        = onDone;               // always call latest closure

    useEffect(() => {
        if (paused) {
            // Snapshot elapsed time so we can resume from here
            if (segStartRef.current !== null) {
                elapsedRef.current += performance.now() - segStartRef.current;
                segStartRef.current = null;
            }
            if (rafRef.current) { cancelAnimationFrame(rafRef.current); rafRef.current = null; }
            return;
        }

        // (Re)start animation from wherever we paused
        segStartRef.current = performance.now();
        doneRef.current     = false;

        const tick = (now: number) => {
            const total = elapsedRef.current + (now - segStartRef.current!);
            const p     = Math.min((total / duration) * 100, 100);
            setPct(p);
            if (p < 100) {
                rafRef.current = requestAnimationFrame(tick);
            } else if (!doneRef.current) {
                doneRef.current = true;
                onDoneRef.current();
            }
        };
        rafRef.current = requestAnimationFrame(tick);
        return () => { if (rafRef.current) cancelAnimationFrame(rafRef.current); };
    }, [paused, duration]);

    return (
        <div className="hiw-progress-track">
            <div className="hiw-progress-fill"
                style={{ width: `${pct}%`, background: "var(--hiw-accent)" }} />
        </div>
    );
}

// ─── Per-slide SVG visuals ────────────────────────────────────────────────────

function SlideVisual({ id, chipColor }: { id: SlideId; chipColor: string }) {
    switch (id) {
        case "problem":  return <VisualProblem  color={chipColor} />;
        case "solution": return <VisualSolution color={chipColor} />;
        case "security": return <VisualSecurity color={chipColor} />;
    }
}

// Shared keyframe styles injected once
const KEYFRAMES = `
@keyframes hiw-pulse  { 0%,100%{opacity:.12} 50%{opacity:.25} }
@keyframes hiw-pulse2 { 0%,100%{opacity:.08} 50%{opacity:.2} }
@keyframes hiw-dash   { from{stroke-dashoffset:20} to{stroke-dashoffset:0} }
@keyframes hiw-float  { 0%,100%{transform:translateY(0)} 50%{transform:translateY(-2px)} }
@keyframes hiw-shake  { 0%,10%,90%,100%{transform:translateX(0)} 30%{transform:translateX(-1.5px)} 70%{transform:translateX(1.5px)} }
@keyframes hiw-appear { from{opacity:0;transform:scale(.88)} to{opacity:1;transform:scale(1)} }
`;

// ── Slide 1: The Problem ─────────────────────────────────────────────────────
function VisualProblem({ color }: { color: string }) {
    // Device node: rounded-rect instead of circle
    const DeviceNode = ({ x, y, type, stroke, delay = 0 }: { x: number; y: number; type: 'laptop' | 'desktop' | 'phone'; stroke: string; delay?: number }) => (
        <g style={{ animation: `hiw-float 5s ease-in-out ${delay}s infinite` }}>
            <rect x={x - 18} y={y - 18} width="36" height="36" rx="7"
                fill="var(--color-bg-card,#1f2937)" stroke={stroke} strokeWidth="1.3" />
            {type === 'laptop' && <>
                <rect x={x - 9} y={y - 7} width="18" height="12" rx="1.5" fill="none" stroke={stroke} strokeWidth="1" opacity="0.7" />
                <line x1={x - 12} y1={y + 7} x2={x + 12} y2={y + 7} stroke={stroke} strokeWidth="1.5" opacity="0.7" />
            </>}
            {type === 'desktop' && <>
                <rect x={x - 10} y={y - 9} width="20" height="14" rx="1.5" fill="none" stroke={stroke} strokeWidth="1" opacity="0.7" />
                <line x1={x} y1={y + 5} x2={x} y2={y + 9} stroke={stroke} strokeWidth="1.5" opacity="0.7" />
                <line x1={x - 5} y1={y + 9} x2={x + 5} y2={y + 9} stroke={stroke} strokeWidth="1.5" opacity="0.7" />
            </>}
            {type === 'phone' && <>
                <rect x={x - 6} y={y - 10} width="12" height="20" rx="2.5" fill="none" stroke={stroke} strokeWidth="1" opacity="0.7" />
                <circle cx={x} cy={y + 7} r="1.2" fill={stroke} opacity="0.7" />
            </>}
        </g>
    );

    return (
        <svg viewBox="0 0 400 280" className="hiw-svg">
            <defs><style>{KEYFRAMES}</style></defs>
            <line x1="200" y1="18" x2="200" y2="258"
                stroke="#374151" strokeWidth="1" strokeDasharray="4 3" opacity="0.45" />

            {/* ══ LEFT: LOCAL ONLY ══ */}
            <text x="100" y="20" textAnchor="middle" fontSize="10"
                fontFamily="monospace" fill="#6b7280" letterSpacing="1.5">LOCAL ONLY</text>
            <ellipse cx="100" cy="128" rx="76" ry="68"
                fill="#1f293710" stroke="#374151" strokeWidth="1.2" strokeDasharray="5 3" />

            <DeviceNode x={70}  y={104} type="laptop"  stroke="#4b5563" delay={0} />
            <DeviceNode x={132} y={104} type="desktop" stroke="#4b5563" delay={0.5} />
            <DeviceNode x={100} y={156} type="phone"   stroke="#4b5563" delay={1} />

            {([[ 88, 104, 114, 104], [72, 120, 92, 140], [130, 120, 110, 140]] as number[][]).map(([x1, y1, x2, y2], i) => (
                <line key={i} x1={x1} y1={y1} x2={x2} y2={y2}
                    stroke="#4b5563" strokeWidth="1.2" strokeDasharray="4 2" opacity="0.8"
                    style={{ animation: `hiw-dash 1.8s linear ${i * 0.3}s infinite` }} />
            ))}

            <text x="100" y="212" textAnchor="middle" fontSize="9.5"
                fontFamily="monospace" fill="#34d399">PRIVATE TALK OK</text>
            {/* crossed globe — no internet */}
            <rect x="87" y="225" width="26" height="26" rx="6" fill="none"
                stroke="#374151" strokeWidth="1.2" opacity="0.6" />
            <line x1="87" y1="225" x2="113" y2="251" stroke={color} strokeWidth="2" />
            <line x1="113" y1="225" x2="87"  y2="251" stroke={color} strokeWidth="2" />
            <text x="100" y="266" textAnchor="middle" fontSize="9.5"
                fontFamily="monospace" fill={color}>NO INTERNET</text>

            {/* ══ RIGHT: WITH INTERNET ══ */}
            <text x="300" y="20" textAnchor="middle" fontSize="10"
                fontFamily="monospace" fill="#6b7280" letterSpacing="1.5">WITH INTERNET</text>

            {/* Globe node */}
            <rect x="276" y="38" width="48" height="48" rx="10"
                fill="var(--color-bg-card,#111827)" stroke="#374151" strokeWidth="1.5" opacity="0.9" />
            {/* globe lines */}
            <circle cx="300" cy="62" r="15" fill="none" stroke="#4b5563" strokeWidth="1.1" />
            <ellipse cx="300" cy="62" rx="7" ry="15" fill="none" stroke="#4b5563" strokeWidth="1" />
            <line x1="285" y1="62" x2="315" y2="62" stroke="#4b5563" strokeWidth="1" />

            {/* Globe → IP banner connector (stops cleanly at the banner's
              * top edge; the previous line ran through the banner). */}
            <line x1="300" y1="86" x2="300" y2="110" stroke="#374151" strokeWidth="1.5" />
            <rect x="234" y="110" width="132" height="16" rx="3"
                fill={color} fillOpacity="0.10" stroke={color} strokeOpacity="0.4" strokeWidth="1" />
            <text x="300" y="122" textAnchor="middle" fontSize="9.5"
                fontFamily="monospace" fill={color}>PUBLIC IP: 203.0.113.42</text>

            {/* IP banner → device connector */}
            <line x1="300" y1="126" x2="300" y2="134" stroke={color} strokeOpacity="0.5" strokeWidth="1.5" />

            {/* Device under attack */}
            <g style={{ animation: "hiw-shake 3.5s ease-in-out infinite" }}>
                <rect x="276" y="134" width="48" height="48" rx="10"
                    fill="var(--color-bg-card,#1f2937)" stroke={color} strokeWidth="1.8"
                    style={{ filter: "drop-shadow(0 0 5px var(--hiw-accent))" }} />
                <rect x="284" y="143" width="24" height="16" rx="2" fill="none" stroke={color} strokeWidth="1" opacity="0.7" />
                <line x1="288" y1="159" x2="312" y2="159" stroke={color} strokeWidth="1.5" opacity="0.7" />
            </g>

            {/* Attacker nodes — warning triangle with !. Moved further apart
              * horizontally so the attack-arrow lines don't overlap with the
              * device under attack or with each other. */}
            {[{ x: 238, y: 222, delay: 0 }, { x: 362, y: 222, delay: 0.7 }].map((a, i) => (
                <g key={i} style={{ animation: `hiw-float 4s ease-in-out ${a.delay}s infinite` }}>
                    <rect x={a.x - 16} y={a.y - 16} width="32" height="32" rx="7"
                        fill="#450a0a" stroke={color} strokeWidth="1.2" opacity="0.95" />
                    {/* Warning triangle */}
                    <path d={`M${a.x},${a.y - 9} l9,15 h-18 z`}
                        fill={color} fillOpacity="0.25" stroke={color} strokeWidth="1.2" strokeLinejoin="round" />
                    {/* Exclamation inside triangle */}
                    <line x1={a.x} y1={a.y - 3} x2={a.x} y2={a.y + 4} stroke={color} strokeWidth="1.5" strokeLinecap="round" />
                    <circle cx={a.x} cy={a.y + 7} r="0.8" fill={color} />
                </g>
            ))}
            {/* Attack arrows — start outside attacker rect top edge, end
              * clearly at the device's bottom-left / bottom-right corner so
              * the lines read as "strikes" rather than passing through. */}
            <line x1="254" y1="206" x2="280" y2="184" stroke={color} strokeWidth="1.5" strokeDasharray="4 2" opacity="0.6"
                style={{ animation: "hiw-dash 1.6s linear infinite" }} />
            <line x1="346" y1="206" x2="320" y2="184" stroke={color} strokeWidth="1.5" strokeDasharray="4 2" opacity="0.6"
                style={{ animation: "hiw-dash 1.6s linear 0.4s infinite" }} />

            <text x="300" y="262" textAnchor="middle" fontSize="9.5"
                fontFamily="monospace" fill={color}>DDoS  ·  PORT SCAN  ·  ATTACKS</text>
        </svg>
    );
}

// ── Slide 2: Solution — animated mesh hub with flowing data packets ─────────
function VisualSolution({ color }: { color: string }) {
    const SX = 200, SY = 168;   // shield / mesh hub centre

    // Three device nodes fanned across the upper arc
    const devices = [
        { x: 72,  y: 88, icon: "💻", label: "HOME",   ip: "100.x.x.x", delay: "0s",    dur: "1.9s" },
        { x: 200, y: 55, icon: "📱", label: "MOBILE",  ip: "100.x.x.x", delay: "0.6s", dur: "1.6s" },
        { x: 328, y: 88, icon: "🖥️", label: "OFFICE",  ip: "100.x.x.x", delay: "1.1s", dur: "2.1s" },
    ];

    return (
        <svg viewBox="0 0 400 285" className="hiw-svg">
            <defs>
                <style>{KEYFRAMES}</style>
                {/* Radial gradient for shield fill */}
                <radialGradient id="hiw-shield-grad" cx="50%" cy="50%" r="50%">
                    <stop offset="0%"   stopColor="var(--hiw-accent)" stopOpacity="0.18" />
                    <stop offset="100%" stopColor="var(--hiw-accent)" stopOpacity="0.03" />
                </radialGradient>
            </defs>

            {/* ── Subtle background lattice ── */}
            {[100, 200, 300].map(x => (
                <line key={x} x1={x} y1="0" x2={x} y2="285"
                    stroke="#374151" strokeWidth="0.4" opacity="0.18" strokeDasharray="2 8" />
            ))}
            {[80, 160, 240].map(y => (
                <line key={y} x1="0" y1={y} x2="400" y2={y}
                    stroke="#374151" strokeWidth="0.4" opacity="0.18" strokeDasharray="2 8" />
            ))}

            {devices.map((d, i) => {
                // Path string from device center → shield center
                const path = `M ${d.x},${d.y} L ${SX},${SY}`;
                // Midpoint for the 🔐 lock badge
                const mx = (d.x + SX) / 2;
                const my = (d.y + SY) / 2;

                return (
                    <g key={i}>
                        {/* Dashed tunnel line */}
                        <line x1={d.x} y1={d.y} x2={SX} y2={SY}
                            stroke={color} strokeWidth="1.5" strokeOpacity="0.35"
                            strokeDasharray="7 4"
                            style={{ animation: `hiw-dash ${d.dur} linear ${d.delay} infinite` }} />

                        {/* Animated data packet (dot moving toward shield) */}
                        <circle r="3" fill={color} fillOpacity="0.90">
                            <animateMotion dur={d.dur} begin={d.delay} repeatCount="indefinite"
                                path={path} />
                        </circle>

                        {/* Lock badge at midpoint */}
                        <text x={mx} y={my + 5} textAnchor="middle" fontSize="12"
                            style={{ animation: `hiw-float ${2.8 + i * 0.4}s ease-in-out ${i * 0.3}s infinite` }}>
                            🔐
                        </text>

                        {/* Device node */}
                        <g style={{ animation: `hiw-float ${3 + i * 0.5}s ease-in-out ${i * 0.4}s infinite` }}>
                            {/* Outer ring glow */}
                            <circle cx={d.x} cy={d.y} r="30"
                                fill="none" stroke={color} strokeWidth="1" strokeOpacity="0.18"
                                style={{ animation: `hiw-pulse2 ${3.5 + i * 0.3}s ease-in-out ${i * 0.5}s infinite` }} />
                            {/* Main circle */}
                            <circle cx={d.x} cy={d.y} r="22"
                                fill="var(--color-bg-card,#111827)"
                                stroke={color} strokeWidth="1.6" strokeOpacity="0.80"
                                style={{ filter: "drop-shadow(0 0 8px var(--hiw-accent))" }} />
                            <text x={d.x} y={d.y + 8} textAnchor="middle" fontSize="14">{d.icon}</text>
                        </g>

                        {/* Location label above device */}
                        <text x={d.x} y={d.y - 28} textAnchor="middle" fontSize="8.5"
                            fontFamily="monospace" fill="#9ca3af" letterSpacing="1.8">
                            {d.label}
                        </text>

                        {/* Private IP tag below device */}
                        <rect x={d.x - 38} y={d.y + 27} width="76" height="13" rx="2.5"
                            fill={color} fillOpacity="0.09"
                            stroke={color} strokeOpacity="0.32" strokeWidth="1" />
                        <text x={d.x} y={d.y + 37} textAnchor="middle" fontSize="8"
                            fontFamily="monospace" fill={color}>{d.ip}</text>
                    </g>
                );
            })}

            {/* ── Private Mesh hub (centre) ── */}
            {/* Outermost slow-pulse ring */}
            <circle cx={SX} cy={SY} r="60"
                fill="none" stroke={color} strokeWidth="1" strokeOpacity="0.08"
                style={{ animation: "hiw-pulse 4s ease-in-out infinite" }} />
            {/* Mid ring */}
            <circle cx={SX} cy={SY} r="46"
                fill="none" stroke={color} strokeWidth="1" strokeOpacity="0.13"
                style={{ animation: "hiw-pulse2 3s ease-in-out 0.5s infinite" }} />
            {/* Main filled circle */}
            <circle cx={SX} cy={SY} r="36"
                fill="url(#hiw-shield-grad)"
                stroke={color} strokeWidth="2" strokeOpacity="0.75"
                style={{ filter: "drop-shadow(0 0 14px var(--hiw-accent))" }} />
            <text x={SX} y={SY + 10} textAnchor="middle" fontSize="26">🛡️</text>
            <text x={SX} y={SY + 27} textAnchor="middle" fontSize="8"
                fontFamily="monospace" fill={color} letterSpacing="1.6">PRIVATE MESH</text>

            {/* ── Shield → Internet line ── */}
            <line x1={SX} y1={SY + 38} x2={SX} y2={248}
                stroke={color} strokeWidth="1.5" strokeOpacity="0.38" strokeDasharray="5 3"
                style={{ animation: "hiw-dash 1.8s linear 0.9s infinite" }} />

            {/* Data packet from shield to internet */}
            <circle r="3" fill={color} fillOpacity="0.85">
                <animateMotion dur="1.8s" begin="0.9s" repeatCount="indefinite"
                    path={`M ${SX},${SY + 38} L ${SX},248`} />
            </circle>

            {/* ── Internet globe ── */}
            <g style={{ animation: "hiw-float 3.5s ease-in-out 1s infinite" }}>
                <circle cx={SX} cy={256} r="18"
                    fill="var(--color-bg-card,#111827)" stroke="#4b5563" strokeWidth="1.5" />
                <text x={SX} y={263} textAnchor="middle" fontSize="14">🌐</text>
            </g>

            {/* "No public IP" badge — wide enough for the full text */}
            <rect x={SX - 115} y="271" width="230" height="13" rx="2"
                fill="#15803d22" stroke="#15803d" strokeWidth="0.8" strokeOpacity="0.5" />
            <text x={SX} y="281" textAnchor="middle" fontSize="8"
                fontFamily="monospace" fill="#4ade80" letterSpacing="0.6">✓ INTERNET ACCESS · NO PUBLIC IP EXPOSED</text>
        </svg>
    );
}

// ── Slide 3: Security — encryption details ────────────────────────────────────
function VisualSecurity({ color }: { color: string }) {
    return (
        <svg viewBox="0 0 400 280" className="hiw-svg">
            <defs><style>{KEYFRAMES}</style></defs>

            <text x="200" y="28" textAnchor="middle" fontSize="10"
                fontFamily="monospace" fill="#6b7280" letterSpacing="2">PC TO PC · ENCRYPTED LINK</text>

            {/* Two PCs */}
            {[{ x: 82, y: 92, label: "PC A" }, { x: 318, y: 92, label: "PC B" }].map((d, i) => (
                <g key={i} style={{ animation: `hiw-float 3.2s ease-in-out ${i * 0.6}s infinite` }}>
                    <rect x={d.x - 30} y={d.y - 28} width="60" height="54" rx="11"
                        fill="var(--color-bg-card,#1f2937)" stroke={color} strokeWidth="1.5"
                        style={{ filter: "drop-shadow(0 0 4px var(--hiw-accent))" }} />
                    <rect x={d.x - 16} y={d.y - 14} width="32" height="20" rx="2" fill="none" stroke={color} strokeWidth="1.2" opacity="0.75" />
                    <line x1={d.x} y1={d.y + 7} x2={d.x} y2={d.y + 14} stroke={color} strokeWidth="1.5" opacity="0.75" />
                    <line x1={d.x - 10} y1={d.y + 14} x2={d.x + 10} y2={d.y + 14} stroke={color} strokeWidth="1.5" opacity="0.75" />
                    <text x={d.x} y={d.y + 45} textAnchor="middle" fontSize="9"
                        fontFamily="monospace" fill={color} letterSpacing="1">{d.label}</text>
                </g>
            ))}

            <line x1="112" y1="92" x2="160" y2="92"
                stroke={color} strokeWidth="4" strokeLinecap="round" opacity="0.58" />
            <line x1="240" y1="92" x2="288" y2="92"
                stroke={color} strokeWidth="4" strokeLinecap="round" opacity="0.58" />
            <g style={{ animation: "hiw-dash 1.2s linear infinite" }}>
                {[130, 246].map((x, i) => (
                    <path key={i} d={`M ${x} 92 l 10 0 l -4 -4 M ${x + 10} 92 l -4 4`}
                        fill="none" stroke={color} strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" opacity="0.9" />
                ))}
            </g>

            <g style={{ animation: "hiw-pulse 3s ease-in-out infinite" }}>
                <rect x="164" y="54" width="72" height="76" rx="16"
                    fill="var(--color-bg-card,#1f2937)" fillOpacity="1" stroke={color} strokeWidth="3"
                    style={{ filter: "drop-shadow(0 0 16px var(--hiw-accent))" }} />
                <rect x="181" y="86" width="38" height="28" rx="7" fill={color} fillOpacity="1" stroke={color} strokeWidth="2" />
                <path d="M188,86 v-9 a12,12 0 0,1 24,0 v9" fill="none" stroke={color} strokeWidth="3.5" strokeLinecap="round" />
                <circle cx="200" cy="99" r="4" fill="var(--color-bg-card,#1f2937)" opacity="1" />
                <rect x="198" y="102" width="4" height="8" rx="1.4" fill="var(--color-bg-card,#1f2937)" opacity="1" />
            </g>

            {[
                { y: 180, label: "ENCRYPTION", value: "WireGuard® ChaCha20-Poly1305", delay: 0 },
                { y: 208, label: "HANDSHAKE",  value: "Noise IK  ·  X25519 keys",     delay: 0.15 },
                { y: 236, label: "SCOPE",      value: "End-to-End  ·  No central server", delay: 0.3 },
            ].map((row, i) => (
                <g key={i} style={{ animation: `hiw-appear 0.4s ease-out ${row.delay}s both` }}>
                    <rect x="28" y={row.y - 13} width="344" height="21" rx="5"
                        fill={color} fillOpacity="0.06" stroke={color} strokeOpacity="0.2" strokeWidth="1" />
                    <text x="42" y={row.y} fontSize="8" fontFamily="monospace"
                        fill={color} fontWeight="bold" letterSpacing="1">{row.label}</text>
                    <text x="130" y={row.y} fontSize="9" fontFamily="monospace"
                        fill="var(--color-text-secondary,#9ca3af)">{row.value}</text>
                </g>
            ))}
        </svg>
    );
}
