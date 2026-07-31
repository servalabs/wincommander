import { Card, Tooltip } from "@/components/ui/bp";
import { useState } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { MeshVPNPeer } from "../../hooks/useBackend";
import { panelVariants, panelTransition, DURATION_S, EASE } from "../../components/shared/motion";
import { staggerDelay } from "../../components/shared/AnimatedList";
import './MeshGrid.css';
import { serviceIcons } from '@/assets';

export default function MeshGrid({
    title,
    peers,
    onSendFile,
    activeExitNodeIP,
    staleThresholdMs
}: {
    title: string,
    peers: MeshVPNPeer[],
    onSendFile: (p: MeshVPNPeer) => void,
    activeExitNodeIP?: string,
    staleThresholdMs: number
}) {
    const [offlineExpanded, setOfflineExpanded] = useState(false);

    if (peers.length === 0) return null;
    const now = new Date();

    const onlinePeers  = peers.filter(p => p.Online);
    const offlinePeers = peers.filter(p => !p.Online);

    const renderPeer = (peer: MeshVPNPeer) => {
        const isActiveGateway = activeExitNodeIP && peer.IPs?.some(ip => ip.split('/')[0] === activeExitNodeIP.split('/')[0]);
        const isStale = !peer.Online && peer.LastSeen && (now.getTime() - new Date(peer.LastSeen).getTime() >= staleThresholdMs);
        const online  = !!peer.Online;
        const timeAgo = !online ? longLastSeen(peer.LastSeen) : null;

        return (
            <Card key={peer.ID} className={`mesh-node-card group/card ${!online ? 'idle' : ''} ${isStale ? 'stale' : ''} ${isActiveGateway ? 'ring-2 ring-orange-500/40 bg-orange-500/5' : ''}`}>

                {/* Top row: status LEFT · send icon + OS logo RIGHT */}
                <div className="flex items-center justify-between gap-2">
                    {/* Left: status dot + label only — no action button here */}
                    <div className="flex items-center gap-1.5 min-w-0">
                        <div className={`w-1.5 h-1.5 rounded-full flex-shrink-0 ${online ? 'bg-emerald-500 shadow-[0_0_8px_rgba(16,185,129,0.8)]' : 'bg-[var(--color-text-muted)] opacity-30'}`} />
                        {online && (
                            <span className="text-[10px] font-mono tracking-[0.2em] text-[var(--color-text-muted)] uppercase truncate">
                                {isActiveGateway ? 'ACTIVE GATEWAY' : 'ACTIVE'}
                            </span>
                        )}
                    </div>

                    {/* Right: send button (always visible) + OS logo */}
                    <div className="flex items-center gap-1.5 flex-shrink-0">
                        {online && (
                            <Tooltip content="Send file to this device">
                                <button
                                    type="button"
                                    className="mesh-send-btn"
                                    onClick={() => onSendFile(peer)}
                                    aria-label="Send file"
                                >
                                    <SendFileIcon />
                                </button>
                            </Tooltip>
                        )}
                        <Tooltip content={osTooltip(peer.OS)}>
                            <span className="flex items-center">
                                <OsLogo os={peer.OS} size={24} />
                            </span>
                        </Tooltip>
                    </div>
                </div>

                {/* For offline nodes: relative time shown above the device name */}
                {!online && timeAgo && (
                    <div
                        className="font-mono uppercase tracking-widest mt-3"
                        style={{ fontSize: 9, color: isStale ? 'var(--color-danger)' : 'var(--color-text-muted)', opacity: 0.7 }}
                    >
                        {timeAgo}
                    </div>
                )}

                {/* Device hostname */}
                <div className={`text-sm font-black text-[var(--color-text-primary)] tracking-wider uppercase truncate ${!online && timeAgo ? 'mt-0.5' : 'mt-3'}`}>
                    {peer.Hostname}
                </div>

                {/* Mesh IP address — micro-label + value stacked with clear
                    separation from the hostname above so they never crowd
                    in the narrow xl:grid-cols-4 card. */}
                <div className="mesh-node-ip">
                    <span className="mesh-node-ip__label">IP</span>
                    <span className="mesh-node-ip__value truncate" title={peer.IPs?.[0] || undefined}>
                        {peer.IPs?.[0] || "---"}
                    </span>
                </div>

                {isActiveGateway && (
                    <div className="absolute bottom-0 left-0 right-0 h-1 bg-gradient-to-r from-orange-500 to-transparent" />
                )}
                {(peer.ExitNodeOption || isActiveGateway) && (
                    <div className="mt-3 flex">
                        <span className={`${isActiveGateway ? 'text-orange-400' : 'text-orange-500/60'} text-[8px] font-black uppercase tracking-[0.2em] flex items-center gap-1.5`}>
                            <div className={`w-1 h-1 rounded-full ${isActiveGateway ? 'bg-orange-400' : 'bg-orange-500/40'}`} />
                            {isActiveGateway ? 'ACTIVE MESH GATEWAY' : 'Route Through Node'}
                        </span>
                    </div>
                )}
            </Card>
        );
    };

    return (
        // Replace dead animate-fade-in CSS class with framer panelVariants.
        // WHY: no @keyframes for animate-fade-in existed, so the section and
        // error banner never animated. panelVariants (opacity + y=8→0) is the
        // shared panel-enter treatment; MotionConfig in App.tsx handles
        // reduced-motion automatically.
        <motion.div
            className="mesh-section"
            variants={panelVariants}
            initial="initial"
            animate="animate"
            exit="exit"
            transition={panelTransition}
        >
            <div className="flex items-center gap-4 mb-6 py-2">
                <div className="text-[11px] font-mono font-black text-[var(--color-text-muted)] opacity-65 uppercase tracking-[0.3em]">{title}</div>
                <div className="h-px flex-1 bg-[var(--color-border)]" />
            </div>

            {onlinePeers.length > 0 && (
                // Staggered entrance: each online peer card fades in with an
                // index-based delay capped by staggerDelay so long peer lists
                // never take more than 250ms total.
                <div className="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-4 gap-4">
                    <AnimatePresence initial>
                        {onlinePeers.map((peer, idx) => (
                            <motion.div
                                key={peer.ID}
                                initial={{ opacity: 0, y: 6 }}
                                animate={{ opacity: 1, y: 0 }}
                                exit={{ opacity: 0 }}
                                transition={{
                                    delay: staggerDelay(idx),
                                    duration: DURATION_S.normal,
                                    ease: EASE.enter,
                                }}
                            >
                                {renderPeer(peer)}
                            </motion.div>
                        ))}
                    </AnimatePresence>
                </div>
            )}

            {offlinePeers.length > 0 && (
                <div className={onlinePeers.length > 0 ? 'mt-8' : 'mt-4'} style={{ paddingTop: 8, paddingBottom: 8 }}>
                    <button
                        className="flex items-center gap-2 text-[10px] font-mono text-[var(--color-text-muted)] hover:text-[var(--color-text-secondary)] transition-colors mb-3 px-1 py-2"
                        onClick={() => setOfflineExpanded(e => !e)}
                    >
                        <span style={{ fontSize: 10 }}>{offlineExpanded ? '▾' : '▸'}</span>
                        <span className="uppercase tracking-[0.2em]">{offlinePeers.length} offline device{offlinePeers.length !== 1 ? 's' : ''}</span>
                    </button>
                    {offlineExpanded && (
                        <div className="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-4 gap-4">
                            {offlinePeers.map(renderPeer)}
                        </div>
                    )}
                </div>
            )}
        </motion.div>
    );
}

// ── Send-file icon ───────────────────────────────────────────────────
// Paper-plane shape: universally understood as "send / transmit to device".
function SendFileIcon() {
    return (
        <svg width={12} height={12} viewBox="0 0 24 24" fill="none"
            stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <line x1="22" y1="2" x2="11" y2="13"/>
            <polygon points="22 2 15 22 11 13 2 9 22 2"/>
        </svg>
    );
}

// ── Colored OS logo SVGs ─────────────────────────────────────────────
// Exported so index.tsx can reuse them in gateway chips.
export function OsLogo({ os, size = 16 }: { os: string | null | undefined; size?: number }) {
    const lower = normaliseOs(os);

    // Tailscale reports "darwin" for macOS. It contains the letters "win",
    // so it must be checked before Windows; the former broad Win match made
    // Macs render as Windows devices.
    if (lower.includes('mac') || lower.includes('darwin') || lower.includes('ios') || lower.includes('iphone') || lower.includes('ipad')) {
        return <AppleLogo size={size} />;
    }
    if (lower.includes('android')) return <AndroidLogo size={size} />;
    if (lower.includes('freebsd') || lower.includes('openbsd') || lower.includes('netbsd') || lower.includes('bsd')) {
        return <FreeBSDLogo size={size} />;
    }
    if (lower.includes('linux') || lower.includes('unix')) return <LinuxLogo size={size} />;
    if (lower.includes('windows') || lower === 'win32' || /^win(?:dows)?(?:10|11)?$/.test(lower)) {
        return <WindowsLogo size={size} />;
    }
    return <UnknownOsLogo size={size} />;
}

function normaliseOs(os: string | null | undefined): string {
    return typeof os === 'string' ? os.trim().toLowerCase() : '';
}

/** Never infer an OS from a hostname or IP: neither is reliable or safe.
 *  A peer that has not reported its OS is deliberately shown as unknown. */
export function osTooltip(os: string | null | undefined): string {
    const value = normaliseOs(os);
    return value && !['unknown', 'n/a', 'na', 'none', 'null'].includes(value)
        ? os!.trim()
        : 'Unknown OS — not reported by this device';
}

function WindowsLogo({ size }: { size: number }) {
    // Microsoft Windows 11 logo — four flat squares in Windows blue with
    // identical gaps (vs. Windows 10's slight perspective skew).
    const blue = "#0078d4";
    return (
        <svg width={size} height={size} viewBox="0 0 22 22" style={{ display: 'block' }}>
            <rect x="1"  y="1"  width="9" height="9" fill={blue}/>
            <rect x="12" y="1"  width="9" height="9" fill={blue}/>
            <rect x="1"  y="12" width="9" height="9" fill={blue}/>
            <rect x="12" y="12" width="9" height="9" fill={blue}/>
        </svg>
    );
}

function AppleLogo({ size }: { size: number }) {
    // Uses currentColor so the apple mark is light on dark themes and
    // dark on light themes — always readable.
    return (
        <svg width={size} height={size} viewBox="0 0 16 17"
            style={{ display: 'block', color: 'var(--color-text-secondary)' }}>
            <path
                d="M13.1 12.4c-.5.8-1 1.6-1.8 1.6-.7 0-1-.4-1.9-.4-.9 0-1.2.4-1.9.4-.8 0-1.4-.9-1.9-1.6-1.3-2-1.9-4.4-.9-6.2.5-.9 1.4-1.5 2.5-1.5.8 0 1.4.5 1.9.5s1.3-.6 2.1-.5c.4 0 1.4.2 2 1.2-.1.1-1.2.7-1.2 2.2 0 1.7 1.5 2.4 1.5 2.4s-.2.7-.4 1.9z"
                fill="currentColor"
            />
            <path
                d="M10.2 3c.4-.6 1.1-.9 1.7-.9.1.7-.2 1.5-.6 1.9-.4.5-1 .9-1.7.8-.1-.7.2-1.4.6-1.8z"
                fill="currentColor"
            />
        </svg>
    );
}

function AndroidLogo({ size }: { size: number }) {
    return (
        <img src={serviceIcons['android.png']} width={size} height={size} alt="Android"
            style={{ objectFit: 'contain' }} />
    );
}

function LinuxLogo({ size }: { size: number }) {
    return (
        <img src={serviceIcons['linux.png']} width={size} height={size} alt="Linux"
            style={{ objectFit: 'contain' }} />
    );
}

function FreeBSDLogo({ size }: { size: number }) {
    // FreeBSD "Beastie" mark — official red daemon head. Reduced to a clean
    // silhouette: round face, two horns, two cheeks/tufts, and the
    // signature trident-poking-up cue. Red regardless of theme because the
    // FreeBSD brand colour is fixed.
    const red = "#ab2b28";
    const white = "#ffffff";
    return (
        <svg width={size} height={size} viewBox="0 0 32 32" style={{ display: 'block' }}>
            {/* Horns */}
            <path d="M9 6 L11 2 L12.5 6 Z" fill={red}/>
            <path d="M23 6 L21 2 L19.5 6 Z" fill={red}/>
            {/* Head */}
            <circle cx="16" cy="17" r="10" fill={red}/>
            {/* Inner highlight — keeps the daemon mark readable at small sizes */}
            <ellipse cx="13.5" cy="14" rx="1.6" ry="1.9" fill={white}/>
            <ellipse cx="18.5" cy="14" rx="1.6" ry="1.9" fill={white}/>
            <circle cx="13.5" cy="14.3" r="0.7" fill="#1a1a1a"/>
            <circle cx="18.5" cy="14.3" r="0.7" fill="#1a1a1a"/>
            {/* Smile */}
            <path
                d="M11.5 19 Q16 22.5 20.5 19"
                stroke="#1a1a1a"
                strokeWidth="1.1"
                strokeLinecap="round"
                fill="none"
            />
            {/* Trident tip poking up behind one horn — Beastie's signature prop */}
            <path d="M27 11 L29 5 L31 11 L30 11 L30 14 L28 14 L28 11 Z" fill={red}/>
        </svg>
    );
}

function UnknownOsLogo({ size }: { size: number }) {
    return (
        <svg width={size} height={size} viewBox="0 0 20 20" fill="none" role="img" aria-label="Unknown operating system" style={{ display: 'block' }}>
            <rect x="2" y="3" width="16" height="11" rx="2" stroke="var(--color-text-muted)" strokeWidth="1.35"/>
            <path d="M8 17h4M10 14v3" stroke="var(--color-text-muted)" strokeWidth="1.35" strokeLinecap="round"/>
            <path d="M8.1 7.3c.15-1.1.95-1.8 2.05-1.8 1.13 0 1.98.69 1.98 1.68 0 1.32-1.5 1.54-1.8 2.5" stroke="var(--color-text-secondary)" strokeWidth="1.35" strokeLinecap="round"/>
            <circle cx="10.3" cy="11.6" r=".7" fill="var(--color-text-secondary)"/>
        </svg>
    );
}

// ── Time helpers ─────────────────────────────────────────────────────

/** Full relative time text shown above offline node names.
 *  < 60s → "X sec ago"  |  < 60m → "X min ago"  |  < 24h → "X hr ago"
 *  < 31d → "X days ago" |  < 12mo → "X months ago" | else → "X years ago" */
export function longLastSeen(lastSeen: string | undefined): string {
    if (!lastSeen) return "";
    try {
        const diffMs = Date.now() - new Date(lastSeen).getTime();
        const secs   = Math.floor(diffMs / 1000);
        if (secs < 60)  return `${secs} sec ago`;
        const mins   = Math.floor(secs / 60);
        if (mins < 60)  return `${mins} min ago`;
        const hours  = Math.floor(mins / 60);
        if (hours < 24) return `${hours} ${hours === 1 ? 'hr' : 'hrs'} ago`;
        const days   = Math.floor(hours / 24);
        if (days < 31)  return `${days} ${days === 1 ? 'day' : 'days'} ago`;
        const months = Math.floor(days / 30.5);
        if (months < 12) return `${months} ${months === 1 ? 'month' : 'months'} ago`;
        const years  = Math.floor(months / 12);
        return `${years} ${years === 1 ? 'year' : 'years'} ago`;
    } catch {
        return "";
    }
}
