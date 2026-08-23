// WizardAnimations.tsx
// Premium Canvas-2D animated illustrations for each First-Run-Wizard step.
// Uses HTML5 Canvas with 3-D projection, particle physics, and glow sprites.
// Zero external dependencies.

import React, { useRef, useEffect, useCallback } from 'react';
import { companyLogos, software, saas } from '@/assets/featureLogos';

// ── Constants ────────────────────────────────────────────────────────

const ANIM_H = 480;
const BG_DARK = '#080c12';
const BG_LIGHT = '#f0f4f8';
/** Resolve canvas background per current theme. Called each frame so live
 * theme toggles update on the next paint without re-mounting the component. */
const getBG = () => document.documentElement.classList.contains('light') ? BG_LIGHT : BG_DARK;
const BG = BG_DARK;

/** Shared theme palette for wizard canvas animations. Keeps the design
 * language consistent across all steps: when the canvas bg flips to light
 * (#f0f4f8) the cyan neon used on dark (#080c12) washes out. Every animation
 * should read this each frame instead of hardcoding rgb triples. */
export const wizardPalette = () => {
  const isLight = document.documentElement.classList.contains('light');
  return {
    isLight,
    /** primary neon accent triplet — use in `rgba(${p.accent},${alpha})` */
    accent: isLight ? '0, 140, 180' : '0, 242, 255',
    /** secondary (info/hover) accent */
    accentSoft: isLight ? '10, 110, 150' : '0, 210, 255',
    /** danger/blocked accent — red family already reads well on both bgs */
    danger: isLight ? '220, 38, 38' : '255, 50, 50',
    success: isLight ? '22, 163, 74' : '76, 217, 100',
    /** neutral ink — body text, subtle strokes */
    ink: isLight ? '40, 55, 80' : '210, 220, 235',
    /** muted/low-alpha reference color — for very faint backdrops */
    muted: isLight ? '60, 80, 110' : '150, 170, 200',
    /** alpha multiplier — light bg generally needs 1.6x–2x the alpha of dark
     * bg to achieve comparable contrast. Use like `0.35 * p.alphaBoost`. */
    alphaBoost: isLight ? 1.8 : 1,
    /** stroke-width multiplier for light mode (slightly thicker reads better) */
    strokeBoost: isLight ? 1.25 : 1,
  };
};
/** KT: Cap all canvas loops at 30 fps — uncapped rAF pegs 100% CPU on low-power devices (N100 etc.) causing the app window to freeze/go black. */
const FRAME_MS = 1000 / 30;

// ── Shared utilities ─────────────────────────────────────────────────

function initCanvas(c: HTMLCanvasElement, hOverride?: number, dprCap = 2) {
  try {
    const w = Math.max(c.parentElement?.clientWidth ?? 600, 300);
    const h = hOverride ?? ANIM_H;
    const d = Math.min(window.devicePixelRatio ?? 1, dprCap);
    c.width = w * d;
    c.height = h * d;
    c.style.cssText = `display:block;width:${w}px;height:${h}px`;
    const ctx = c.getContext('2d');
    if (!ctx) {
      console.error('[Canvas] Failed to get 2D context — GPU or memory issue?');
      return { ctx: null as any as CanvasRenderingContext2D, w, h };
    }
    ctx.scale(d, d);
    return { ctx, w, h };
  } catch (e) {
    console.error('[Canvas] initCanvas crash:', e);
    return { ctx: null as any as CanvasRenderingContext2D, w: 0, h: 0 };
  }
}

/** Pre-render a radial glow sprite for fast particle blitting. */
function makeGlow(r: number, g: number, b: number, sz = 64): HTMLCanvasElement {
  const c = document.createElement('canvas');
  c.width = c.height = sz;
  const x = c.getContext('2d')!;
  const half = sz / 2;
  const gr = x.createRadialGradient(half, half, 0, half, half, half);
  gr.addColorStop(0, `rgba(${r},${g},${b},0.55)`);
  gr.addColorStop(0.35, `rgba(${r},${g},${b},0.12)`);
  gr.addColorStop(1, 'transparent');
  x.fillStyle = gr;
  x.fillRect(0, 0, sz, sz);
  return c;
}

function hexPath(ctx: CanvasRenderingContext2D, x: number, y: number, r: number) {
  ctx.beginPath();
  for (let i = 0; i < 6; i++) {
    const a = (Math.PI / 3) * i - Math.PI / 6;
    const px = x + r * Math.cos(a), py = y + r * Math.sin(a);
    i === 0 ? ctx.moveTo(px, py) : ctx.lineTo(px, py);
  }
  ctx.closePath();
}


function clamp(v: number, lo: number, hi: number) { return Math.max(lo, Math.min(hi, v)); }
function rand(lo: number, hi: number) { return lo + Math.random() * (hi - lo); }

const containerStyle: React.CSSProperties = {
  width: '100%',
  height: '100%',
  minHeight: ANIM_H,
  borderRadius: 12,
  overflow: 'hidden',
  // Transparent so the host dialog's theme shows through — each animation's
  // canvas paints its own theme-aware bg via getBG() on every frame.
  background: 'transparent',
};

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Step 0 ▸ ComponentsAnimation – System Schematic Blueprint
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

export interface ComponentsAnimationProps {
  enabledMap: Record<string, boolean>;
  onToggle: (key: string) => void;
}

const COMP_DEFS = [
  { key: 'ps7',  name: 'PowerShell 7 Core',   angle: -Math.PI / 2 },
  { key: 'vpn',  name: 'Mesh VPN Tunnel',      angle: -Math.PI / 5 },
  { key: 'enc',  name: 'Encrypted Volumes',     angle: Math.PI / 10 },
  { key: 'prod', name: 'Productivity Engine',   angle: Math.PI / 1.2 },
  { key: 'vc',   name: 'Universal VC++ Redist', angle: Math.PI + Math.PI / 5 },
] as const;

export function ComponentsAnimation({ enabledMap, onToggle }: ComponentsAnimationProps) {
  const ref = useRef<HTMLCanvasElement>(null);
  const enabledRef = useRef(enabledMap);
  enabledRef.current = enabledMap;

  const handleClick = useCallback((e: React.MouseEvent<HTMLCanvasElement>) => {
    const canvas = ref.current;
    if (!canvas) return;
    const rect = canvas.getBoundingClientRect();
    const mx = (e.clientX - rect.left);
    const my = (e.clientY - rect.top);
    const w = rect.width, h = rect.height;
    const cx = w / 2, cy = h / 2;
    const rX = Math.min(w, 600) < 500 ? 100 : 140;
    const rY = Math.min(h, 480) < 350 ? 70 : 95;
    for (const comp of COMP_DEFS) {
      const nx = cx + Math.cos(comp.angle) * rX;
      const ny = cy + Math.sin(comp.angle) * rY;
      const dist = Math.sqrt((mx - nx) ** 2 + (my - ny) ** 2);
      if (dist < 40) { onToggle(comp.key); return; }
    }
  }, [onToggle]);

  useEffect(() => {
    const canvas = ref.current;
    if (!canvas) return;
    const { ctx, w, h } = initCanvas(canvas, 560);
    const cx = w / 2, cy = h / 2;

    const components = COMP_DEFS;

    interface Node {
      key: string;
      name: string;
      x: number; y: number;
      active: boolean; t: number;
    }

    const nodes: Node[] = components.map(c => ({
      key: c.key,
      name: c.name,
      x: cx + Math.cos(c.angle) * 140,
      y: cy + Math.sin(c.angle) * 95,
      active: false,
      t: 0
    }));

    const pulses: { x: number, y: number, tx: number, ty: number, progress: number, color: string }[] = [];
    const startTime = performance.now() / 1000;

    let activeIdx = 0;
    let raf: number;
    let lastTs = 0;

    const loop = (ts: number) => {
      if (!ctx || ts - lastTs < FRAME_MS) { raf = requestAnimationFrame(loop); return; }
      lastTs = ts;
      const now = performance.now() / 1000;
      const t = now - startTime;
      const dt = 1 / 60;

      ctx.fillStyle = getBG();
      ctx.fillRect(0, 0, w, h);

      // --- Schematic Backdrop ---
      ctx.strokeStyle = 'rgba(0, 242, 255, 0.05)';
      ctx.lineWidth = 1;
      // Large faint hex around core
      hexPath(ctx, cx, cy, 180);
      ctx.stroke();
      // Tech circles
      ctx.beginPath();
      ctx.arc(cx, cy, 220, 0, Math.PI * 2);
      ctx.setLineDash([5, 20]);
      ctx.stroke();
      ctx.setLineDash([]);

      // --- Logic: Node Activation ---
      const cycleTime = 1.2;
      const totalT = t % (nodes.length * cycleTime + 2);
      activeIdx = Math.floor(totalT / cycleTime);

      nodes.forEach((n, i) => {
        n.active = activeIdx >= i && activeIdx < nodes.length;
        if (n.active) n.t = Math.min(n.t + dt * 2, 1);
        else if (activeIdx < i) n.t = 0;
      });

      // Spawn Pulses
      if (activeIdx < nodes.length && Math.random() > 0.8) {
        pulses.push({
          x: cx, y: cy,
          tx: nodes[activeIdx].x,
          ty: nodes[activeIdx].y,
          progress: 0,
          color: '#00f2ff'
        });
      }

      // --- Draw Core Kernel ---
      const corePulse = 0.8 + Math.sin(t * 3) * 0.2;
      ctx.save();
      ctx.translate(cx, cy);
      ctx.rotate(t * 0.2);

      // Outer hex frame
      ctx.strokeStyle = 'rgba(0, 242, 255, 0.35)';
      ctx.lineWidth = 2.5;
      hexPath(ctx, 0, 0, 42 * corePulse);
      ctx.stroke();

      // Inner pulsing core
      ctx.fillStyle = `rgba(0, 242, 255, ${0.12 + corePulse * 0.12})`;
      hexPath(ctx, 0, 0, 30);
      ctx.fill();

      // Kernel Labels
      ctx.font = '900 11px monospace';
      ctx.fillStyle = 'rgba(0, 242, 255, 0.7)';
      ctx.textAlign = 'center';
      ctx.rotate(-t * 0.2); // Counter-rotate text
      ctx.fillText('KERNEL', 0, 4);
      ctx.restore();

      // --- Draw Connections and Pulses ---
      nodes.forEach((n) => {
        ctx.beginPath();
        ctx.moveTo(cx, cy);
        ctx.lineTo(n.x, n.y);
        ctx.strokeStyle = n.active ? `rgba(0, 242, 255, ${0.15 + n.t * 0.35})` : 'rgba(255, 255, 255, 0.05)';
        ctx.lineWidth = n.active ? 2 : 1;
        ctx.stroke();
      });

      for (let i = pulses.length - 1; i >= 0; i--) {
        const p = pulses[i];
        p.progress += dt * 1.5;
        if (p.progress >= 1) { pulses.splice(i, 1); continue; }

        const px = p.x + (p.tx - p.x) * p.progress;
        const py = p.y + (p.ty - p.y) * p.progress;

        ctx.fillStyle = p.color;
        ctx.beginPath();
        ctx.arc(px, py, 3, 0, Math.PI * 2);
        ctx.fill();
        // trail
        ctx.strokeStyle = p.color;
        ctx.globalAlpha = 0.35 * (1 - p.progress);
        ctx.lineWidth = 2;
        ctx.beginPath();
        ctx.moveTo(px, py);
        ctx.lineTo(p.x + (p.tx - p.x) * (p.progress - 0.1), p.y + (p.ty - p.y) * (p.progress - 0.1));
        ctx.stroke();
        ctx.globalAlpha = 1;
      }

      // --- Draw Satellite Nodes ---
      const eMap = enabledRef.current;
      nodes.forEach((n) => {
        const isEnabled = eMap[n.key] !== false;
        ctx.save();
        ctx.translate(n.x, n.y);

        // Hit area visual (faint hover circle)
        ctx.fillStyle = 'rgba(255,255,255,0.015)';
        ctx.beginPath();
        ctx.arc(0, 0, 36, 0, Math.PI * 2);
        ctx.fill();

        // Node Dot — green if enabled, dim red if disabled
        const nodeColor = isEnabled ? '#00f2ff' : '#ff4466';
        const dimColor = isEnabled ? 'rgba(0,242,255,0.15)' : 'rgba(255,68,102,0.15)';
        ctx.fillStyle = n.active ? nodeColor : dimColor;
        ctx.beginPath();
        ctx.arc(0, 0, 8, 0, Math.PI * 2);
        ctx.fill();

        if (n.active && isEnabled) {
          ctx.strokeStyle = nodeColor;
          ctx.lineWidth = 1.5;
          ctx.beginPath();
          ctx.arc(0, 0, 14 * (1 + Math.sin(t * 10) * 0.1), 0, Math.PI * 2);
          ctx.stroke();
        }

        // Callout Line
        const lineDir = n.x > cx ? 1 : -1;
        ctx.strokeStyle = n.active ? nodeColor : 'rgba(255,255,255,0.08)';
        ctx.lineWidth = n.active ? 1.5 : 1;
        ctx.beginPath();
        ctx.moveTo(lineDir * 10, 0);
        ctx.lineTo(lineDir * 28, -18);
        ctx.lineTo(lineDir * 120, -18);
        ctx.stroke();

        // Label
        ctx.font = '800 14px monospace';
        ctx.fillStyle = isEnabled ? (n.active ? '#fff' : 'rgba(255,255,255,0.5)') : 'rgba(255,68,102,0.5)';
        ctx.textAlign = lineDir === 1 ? 'left' : 'right';
        ctx.fillText(n.name.toUpperCase(), lineDir * 32, -24);

        // Status label
        ctx.font = '700 10px monospace';
        if (isEnabled) {
          ctx.fillStyle = n.active ? 'rgba(0, 242, 255, 0.8)' : 'rgba(0,242,255,0.35)';
          ctx.fillText(n.active ? '✓ ENABLED' : '✓ READY', lineDir * 32, -10);
        } else {
          ctx.fillStyle = 'rgba(255,68,102,0.6)';
          ctx.fillText('✕ SKIPPED', lineDir * 32, -10);
        }

        ctx.restore();
      });

      raf = requestAnimationFrame(loop);
    };
    raf = requestAnimationFrame(loop);
    return () => cancelAnimationFrame(raf);
  }, []);

  return (
    <div style={containerStyle}>
      <canvas ref={ref} onClick={handleClick} style={{ cursor: 'pointer' }} />
    </div>
  );
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Step 1 ▸ ShieldAnimation – Hex Force-Field Deflecting Threats
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

interface Spark { x: number; y: number; vx: number; vy: number; life: number; }
interface Ripple { x: number; y: number; r: number; alpha: number; }

export interface ShieldAnimationProps {
  protectionLevel: 'standard' | 'family';
}

const STANDARD_THREATS = ['🦠', '📢', '🕷️', '⚠️', '💣', '🔓'];
const FAMILY_EXTRA     = ['🔞', '🎰', '💊', '💀'];

export function ShieldAnimation({ protectionLevel }: ShieldAnimationProps) {
  const ref = useRef<HTMLCanvasElement>(null);
  const levelRef = useRef(protectionLevel);
  levelRef.current = protectionLevel;

  useEffect(() => {
    const canvas = ref.current;
    if (!canvas) return;
    const { ctx, w, h } = initCanvas(canvas);
    // Center the shield+laptop combo properly
    const shieldX = w / 2, shieldY = h / 2, shieldR = 82;
    const cyanG = makeGlow(0, 242, 255);
    const whiteG = makeGlow(255, 255, 255, 32);

    // hex grid within shield radius
    const hexSz = 11;
    const hexes: { x: number; y: number; d: number }[] = [];
    for (let row = -8; row <= 8; row++) {
      for (let col = -8; col <= 8; col++) {
        const hx = col * hexSz * 1.55;
        const hy = row * hexSz * Math.sqrt(3) + (col % 2 ? hexSz * Math.sqrt(3) / 2 : 0);
        const d = Math.sqrt(hx * hx + hy * hy);
        if (d < shieldR - 4) hexes.push({ x: hx, y: hy, d });
      }
    }

    interface Threat {
      x: number; y: number; vx: number; vy: number;
      alive: boolean; bounced: boolean; passed: boolean; life: number;
      label: string; isExtra: boolean;
    }

    const threats: Threat[] = [];
    const sparks: Spark[] = [];
    const ripples: Ripple[] = [];
    let nextSpawn = 0;

    // Laptop is INSIDE the shield, centered
    const laptopX = shieldX, laptopY = shieldY + 5;

    let raf: number;
    let lastTs = 0;
    const startTime = performance.now() / 1000;

    const loop = (ts: number) => {
      if (!ctx || ts - lastTs < FRAME_MS) { raf = requestAnimationFrame(loop); return; }
      lastTs = ts;
      const now = performance.now() / 1000;
      const t = now - startTime;
      const dt = 1 / 60;

      const isLight = document.documentElement.classList.contains('light');
      ctx.fillStyle = getBG();
      ctx.fillRect(0, 0, w, h);

      // Theme-aware stroke/fill palette. Dark bg uses neon cyan on low alpha
      // because the dark backdrop gives natural contrast; light bg needs
      // stronger, deeper blues so the laptop, shield hexes, and screen lines
      // don't wash out against #f0f4f8.
      const accent = isLight ? '0, 140, 180' : '0, 242, 255';
      const laptopLine = isLight ? 'rgba(15, 85, 130, 0.85)' : 'rgba(100, 180, 220, 0.6)';
      const laptopScreen = isLight ? 'rgba(20, 35, 60, 0.95)' : 'rgba(6, 12, 20, 0.9)';
      const laptopBase = isLight ? 'rgba(15, 85, 130, 0.7)' : 'rgba(100, 180, 220, 0.4)';
      const hexAlphaBase = isLight ? 0.3 : 0.12;
      const hexAlphaSpan = isLight ? 0.55 : 0.35;

      // spawn
      if (t > nextSpawn) {
        const level = levelRef.current;
        const allEmojis = level === 'family'
          ? [...STANDARD_THREATS, ...FAMILY_EXTRA]
          : [...STANDARD_THREATS, ...FAMILY_EXTRA];
        const count = Math.random() > 0.7 ? 2 : 1;
        for (let i = 0; i < count; i++) {
          const label = allEmojis[Math.floor(Math.random() * allEmojis.length)];
          const isExtra = FAMILY_EXTRA.includes(label);
          threats.push({
            x: -20,
            y: shieldY + rand(-100, 100),
            vx: rand(280, 520),
            vy: rand(-60, 60),
            alive: true, bounced: false, passed: false, life: 1,
            label, isExtra,
          });
        }
        nextSpawn = t + rand(0.08, 0.22);
      }

      // update threats
      const level = levelRef.current;
      for (const th of threats) {
        if (!th.alive) continue;
        th.x += th.vx * dt;
        th.y += th.vy * dt;
        if (th.bounced) {
          th.life -= dt * 2.5;
          if (th.life <= 0) th.alive = false;
        } else if (th.passed) {
          th.life -= dt * 1.2;
          if (th.life <= 0 || th.x > w + 20) th.alive = false;
        } else {
          const dx = th.x - shieldX, dy = th.y - shieldY;
          const d = Math.sqrt(dx * dx + dy * dy);
          if (d < shieldR + 4) {
            // Extra threats (18+, gambling, drugs) pass through on Standard
            if (th.isExtra && level === 'standard') {
              th.passed = true;
              // keep moving forward
            } else {
              th.bounced = true;
              const nx = dx / d, ny = dy / d;
              th.vx = nx * 220 + rand(-60, 60);
              th.vy = ny * 220 + rand(-60, 60);
              ripples.push({ x: th.x, y: th.y, r: 0, alpha: 0.9 });
              for (let k = 0; k < 18; k++) {
                const a = Math.random() * Math.PI * 2;
                sparks.push({
                  x: th.x, y: th.y,
                  vx: Math.cos(a) * rand(60, 160),
                  vy: Math.sin(a) * rand(60, 160),
                  life: 1,
                });
              }
            }
          }
        }
      }

      for (const s of sparks) { s.x += s.vx * dt; s.y += s.vy * dt; s.life -= dt * 3; }
      for (const r of ripples) { r.r += dt * 90; r.alpha -= dt * 2; }

      // -- draw laptop INSIDE shield (before hex overlay) --
      ctx.strokeStyle = laptopLine;
      ctx.lineWidth = 1.5;
      // screen
      ctx.fillStyle = laptopScreen;
      ctx.beginPath();
      ctx.roundRect(laptopX - 28, laptopY - 20, 56, 36, 3);
      ctx.fill();
      ctx.stroke();
      // screen content lines
      for (let i = 0; i < 3; i++) {
        const floatPulse = Math.sin(t * 2 + i) * 0.1;
        const screenLineAlpha = (isLight ? 0.75 : 0.35) + floatPulse;
        ctx.fillStyle = `rgba(${accent},${screenLineAlpha})`;
        ctx.fillRect(laptopX - 20, laptopY - 12 + i * 10, 24 + (i === 1 ? -8 : 0), 3);
      }
      // base
      ctx.fillStyle = laptopBase;
      ctx.beginPath();
      ctx.roundRect(laptopX - 34, laptopY + 16, 68, 5, 2);
      ctx.fill();

      // -- draw shield --
      const pulse = 0.65 + Math.sin(t * 2) * 0.35;

      // shield glow
      const gs = shieldR * 3.5;
      ctx.globalAlpha = 0.45 * pulse;
      ctx.drawImage(cyanG, shieldX - gs / 2, shieldY - gs / 2, gs, gs);
      ctx.globalAlpha = 1;

      // hex cells
      for (const hp of hexes) {
        const frac = 1 - hp.d / shieldR;
        const a = (hexAlphaBase + frac * hexAlphaSpan) * pulse;
        hexPath(ctx, shieldX + hp.x, shieldY + hp.y, hexSz * 0.82);
        ctx.strokeStyle = `rgba(${accent},${a})`;
        ctx.lineWidth = isLight ? 0.9 : 0.7;
        ctx.stroke();
      }

      // shield circle
      ctx.beginPath();
      ctx.arc(shieldX, shieldY, shieldR, 0, Math.PI * 2);
      ctx.strokeStyle = `rgba(${accent},${(isLight ? 0.95 : 0.85) * pulse})`;
      ctx.lineWidth = isLight ? 3 : 2.5;
      ctx.stroke();

      // ripples
      for (const r of ripples) {
        if (r.alpha <= 0) continue;
        ctx.beginPath();
        ctx.arc(r.x, r.y, r.r, 0, Math.PI * 2);
        ctx.strokeStyle = `rgba(${accent},${r.alpha})`;
        ctx.lineWidth = isLight ? 1.8 : 1.5;
        ctx.stroke();
      }

      // threats – rendered as emoji icons
      ctx.font = '26px serif';
      ctx.textAlign = 'center';
      ctx.textBaseline = 'middle';
      for (const th of threats) {
        if (!th.alive) continue;
        const a = th.bounced ? th.life : (th.passed ? th.life : 1);
        ctx.globalAlpha = a;
        ctx.fillText(th.label, th.x, th.y);
        // Show ✓ for passed-through threats
        if (th.passed && th.life > 0.3) {
          ctx.font = '10px sans-serif';
          ctx.fillStyle = '#4ed99c';
          ctx.fillText('✓', th.x, th.y - 18);
          ctx.font = '26px serif';
        }
      }
      ctx.globalAlpha = 1;
      ctx.textAlign = 'left';
      ctx.textBaseline = 'alphabetic';

      // sparks
      for (const s of sparks) {
        if (s.life <= 0) continue;
        ctx.globalAlpha = s.life;
        const ss = 6;
        ctx.drawImage(whiteG, s.x - ss / 2, s.y - ss / 2, ss, ss);
        ctx.beginPath();
        ctx.arc(s.x, s.y, 1, 0, Math.PI * 2);
        ctx.fillStyle = '#aef';
        ctx.fill();
      }
      ctx.globalAlpha = 1;

      // cleanup
      for (let i = threats.length - 1; i >= 0; i--) if (!threats[i].alive) threats.splice(i, 1);
      for (let i = sparks.length - 1; i >= 0; i--) if (sparks[i].life <= 0) sparks.splice(i, 1);
      for (let i = ripples.length - 1; i >= 0; i--) if (ripples[i].alpha <= 0) ripples.splice(i, 1);

      raf = requestAnimationFrame(loop);
    };
    raf = requestAnimationFrame(loop);
    return () => cancelAnimationFrame(raf);
  }, []);

  const localStyle = { ...containerStyle, minHeight: 560 };
  return <div style={localStyle}><canvas ref={ref} /></div>;
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Step 2 ▸ OptimizationAnimation – RAM & CPU Reduction
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

export function OptimizationAnimation() {
  const ref = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = ref.current;
    if (!canvas) return;
    const { ctx, w, h } = initCanvas(canvas);
    const cx = w / 2, cy = h / 2;

    const startTime = performance.now() / 1000;
    const duration = 7; // 5s animate + 2s hold

    let raf: number;
    let lastTs = 0;
    const loop = (ts: number) => {
      if (!ctx || ts - lastTs < FRAME_MS) { raf = requestAnimationFrame(loop); return; }
      lastTs = ts;
      const now = performance.now() / 1000;
      const t = (now - startTime) % duration;
      const progress = Math.min(t / 5, 1); // Animate for 5s, hold for 2s

      ctx.fillStyle = getBG();
      ctx.fillRect(0, 0, w, h);

      // Values
      const ramStart = 5.5, ramEnd = 3.5;
      const cpuStart = 250, cpuEnd = 120;

      const currentRam = ramStart - (ramStart - ramEnd) * progress;
      const currentCpu = Math.floor(cpuStart - (cpuStart - cpuEnd) * progress);

      // --- Background HUD Details ---
      ctx.strokeStyle = 'rgba(0, 242, 255, 0.05)';
      ctx.lineWidth = 1;
      ctx.beginPath();
      ctx.moveTo(0, cy); ctx.lineTo(w, cy);
      ctx.moveTo(cx, 0); ctx.lineTo(cx, h);
      ctx.stroke();

      // --- RAM Gauge (Left) ---
      const ramColor = progress > 0.8 ? '#4ed99c' : (progress > 0.4 ? '#f7c26b' : '#ff4444');
      ctx.save();
      ctx.translate(cx - 160, cy);

      // Bar Background
      ctx.fillStyle = 'rgba(255,255,255,0.05)';
      ctx.fillRect(-22, -90, 44, 180);

      // Liquid Fill
      const fillHeight = 180 * (currentRam / ramStart);
      const grad = ctx.createLinearGradient(0, 90, 0, 90 - fillHeight);
      grad.addColorStop(0, ramColor + '33');
      grad.addColorStop(1, ramColor);
      ctx.fillStyle = grad;
      ctx.fillRect(-22, 90 - fillHeight, 44, fillHeight);

      // Label
      ctx.font = '900 28px monospace';
      ctx.fillStyle = ramColor;
      ctx.textAlign = 'center';
      ctx.fillText(currentRam.toFixed(1) + 'GB', 0, 125);
      ctx.font = '700 13px monospace';
      ctx.fillStyle = 'rgba(255,255,255,0.5)';
      ctx.fillText('RAM USAGE', 0, -105);

      ctx.restore();

      // --- CPU Processes (Right) ---
      const cpuColor = progress > 0.9 ? '#4ed99c' : (progress > 0.5 ? '#f7c26b' : '#ff4444');
      ctx.save();
      ctx.translate(cx + 160, cy);

      // Glow background for text
      ctx.shadowColor = cpuColor;
      ctx.shadowBlur = progress < 1 ? 15 : 0;

      ctx.font = '900 56px monospace';
      ctx.textAlign = 'center';
      ctx.fillStyle = cpuColor;
      ctx.fillText(currentCpu.toString(), 0, 15);
      ctx.shadowBlur = 0;

      // Secondary label
      ctx.font = '700 13px monospace';
      ctx.fillStyle = 'rgba(255,255,255,0.5)';
      ctx.fillText('CPU PROCESSES', 0, -105);

      // Process grid - Visual representation of background noise
      const cols = 10, rows = 15;
      const totalCells = cols * rows;
      for (let i = 0; i < totalCells; i++) {
        const row = Math.floor(i / cols);
        const col = i % cols;
        const isActive = (i / totalCells) < (currentCpu / cpuStart);

        ctx.globalAlpha = isActive ? 0.8 : 0.1;
        ctx.fillStyle = isActive ? (i < (cpuEnd / cpuStart * totalCells) ? '#4ed99c' : '#ff4444') : 'rgba(255,255,255,0.05)';
        ctx.fillRect(col * 12 - 60, row * 6 + 40, 8, 4);
      }
      ctx.globalAlpha = 1;
      ctx.restore();

      ctx.font = '900 16px monospace';
      ctx.textAlign = 'center';
      if (progress < 1) {
        ctx.fillStyle = 'rgba(255,255,255,0.8)';
        ctx.fillText('TUNING SYSTEM PERFORMANCE' + '.'.repeat(Math.floor(t * 4) % 4), cx, cy - 160);
      } else {
        ctx.fillStyle = '#4ed99c';
        ctx.fillText('PEAK STABILITY ACHIEVED', cx, cy - 180);
      }

      raf = requestAnimationFrame(loop);
    };
    raf = requestAnimationFrame(loop);
    return () => cancelAnimationFrame(raf);
  }, []);

  return (
    <div style={containerStyle}>
      <canvas ref={ref} />
    </div>
  );
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Step 3 ▸ RocketAnimation – Warp-Speed Particle Tunnel
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

export function RocketAnimation() {
  const ref = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = ref.current;
    if (!canvas) return;
    const { ctx, w, h } = initCanvas(canvas);
    const cx = w / 2, cy = h / 2;
    const cyanG = makeGlow(0, 242, 255, 96);

    const COUNT = 300;
    const DEPTH = 1600;
    interface WarpStar { x: number; y: number; z: number; pz: number; }
    const stars: WarpStar[] = Array.from({ length: COUNT }, () => {
      const z = rand(0, DEPTH);
      return { x: rand(-w, w), y: rand(-h, h), z, pz: z };
    });

    const RINGS = 6;
    const startTime = performance.now() / 1000;

    let raf: number;
    let lastTs = 0;
    const loop = (ts: number) => {
      if (!ctx || ts - lastTs < FRAME_MS) { raf = requestAnimationFrame(loop); return; }
      lastTs = ts;
      const t = performance.now() / 1000;
      const elapsed = t - startTime;
      // Accelerate over time: starts slow, ramps up, cycles
      const accelPhase = (elapsed % 8) / 8; // 0→1 over 8s
      const easedAccel = accelPhase < 0.7
        ? accelPhase / 0.7 // ramp up 0→1
        : 1 - (accelPhase - 0.7) / 0.3 * 0.3; // ease back a bit
      const speed = 3 + easedAccel * 22;

      // semi-transparent clear for motion-trail
      ctx.fillStyle = 'rgba(8,12,18,0.25)';
      ctx.fillRect(0, 0, w, h);

      // central glow
      ctx.globalAlpha = 0.25;
      ctx.drawImage(cyanG, cx - 48, cy - 48, 96, 96);
      ctx.globalAlpha = 1;

      // tunnel rings
      for (let i = 0; i < RINGS; i++) {
        const rz = ((elapsed * 400 + i * (DEPTH / RINGS)) % DEPTH);
        const s = 500 / (500 + rz);
        const rr = 250 * s;
        ctx.beginPath();
        ctx.arc(cx, cy, rr, 0, Math.PI * 2);
        ctx.strokeStyle = `rgba(0,242,255,${0.06 * s})`;
        ctx.lineWidth = 1;
        ctx.stroke();
      }

      // warp stars
      for (const star of stars) {
        star.pz = star.z;
        star.z -= speed;
        if (star.z <= 0) {
          star.x = rand(-w, w);
          star.y = rand(-h, h);
          star.z = DEPTH;
          star.pz = DEPTH;
        }

        const s1 = 500 / star.z;
        const s2 = 500 / star.pz;
        const sx = star.x * s1 + cx;
        const sy = star.y * s1 + cy;
        const px = star.x * s2 + cx;
        const py = star.y * s2 + cy;

        if (sx < -20 || sx > w + 20 || sy < -20 || sy > h + 20) continue;

        const brightness = clamp(1 - star.z / DEPTH, 0, 1);
        const thick = 0.5 + brightness * 2.5;

        // streak line
        ctx.beginPath();
        ctx.moveTo(px, py);
        ctx.lineTo(sx, sy);
        const grad = ctx.createLinearGradient(px, py, sx, sy);
        grad.addColorStop(0, 'transparent');
        grad.addColorStop(1, `rgba(0,242,255,${brightness * 0.9})`);
        ctx.strokeStyle = grad;
        ctx.lineWidth = thick;
        ctx.stroke();

        // head dot
        if (brightness > 0.3) {
          ctx.beginPath();
          ctx.arc(sx, sy, thick * 0.6, 0, Math.PI * 2);
          ctx.fillStyle = `rgba(200,250,255,${brightness})`;
          ctx.fill();
        }
      }

      // centre flare
      const fr = 4 + Math.sin(t * 3) * 2;
      ctx.beginPath();
      ctx.arc(cx, cy, fr, 0, Math.PI * 2);
      ctx.fillStyle = '#fff';
      ctx.shadowColor = '#00f2ff';
      ctx.shadowBlur = 25;
      ctx.fill();
      ctx.shadowBlur = 0;

      raf = requestAnimationFrame(loop);
    };
    raf = requestAnimationFrame(loop);
    return () => cancelAnimationFrame(raf);
  }, []);

  return <div style={containerStyle}><canvas ref={ref} /></div>;
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Step 3 ▸ AnnoyancesAnimation – Order-From-Chaos Blocks
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

export function AnnoyancesAnimation() {
  const ref = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = ref.current;
    if (!canvas) return;
    const { ctx, w, h } = initCanvas(canvas);
    const cyanG = makeGlow(0, 242, 255);

    // File data: name, extension, isHidden
    const fileData: Array<{ name: string; ext: string; hidden: boolean }> = [
      { name: 'Setup', ext: '.exe', hidden: false },
      { name: 'Document', ext: '.pdf', hidden: false },
      { name: 'Report', ext: '.doc', hidden: false },
      { name: 'Photo', ext: '.jpg', hidden: false },
      { name: 'Config', ext: '.ini', hidden: true },
      { name: 'System', ext: '.dll', hidden: true },
      { name: 'AppData', ext: '', hidden: true },
      { name: 'Backup', ext: '.bak', hidden: false },
    ];

    interface FileItem {
      y: number;
      name: string;
      ext: string;
      hidden: boolean;
    }

    const listX = w / 2 - 80;
    const lineHeight = 22;
    // Ensure all files stay within safe bounds (18px from edges)
    const startY = Math.max(18, (h - fileData.length * lineHeight) / 2);

    const files: FileItem[] = fileData.map((file, i) => {
      const cy = startY + i * lineHeight;
      return {
        y: cy,
        name: file.name,
        ext: file.ext,
        hidden: file.hidden,
      };
    });

    const CYCLE = 7;
    const startTime = performance.now() / 1000;

    let raf: number;
    let lastTs = 0;
    const loop = (ts: number) => {
      if (!ctx || ts - lastTs < FRAME_MS) { raf = requestAnimationFrame(loop); return; }
      lastTs = ts;
      const now = performance.now() / 1000;
      const t = now - startTime;
      const phase = t % CYCLE;

      ctx.fillStyle = getBG();
      ctx.fillRect(0, 0, w, h);

      // Title header
      ctx.font = '600 9px sans-serif';
      ctx.fillStyle = 'rgba(0,242,255,0.3)';
      ctx.textAlign = 'left';
      ctx.textBaseline = 'top';
      ctx.fillText('📁 Files', listX, startY - 25);

      // Extensions appear phase (optional, can keep or remove)
      // Simplifying: always show extensions but maybe fade them slightly?
      // Keeping original "reveal" logic for consistency but without shuffling.
      const extReveal = phase > 1.5 ? clamp((phase - 1.5) * 2, 0, 1) : 0;
      const showExt = extReveal > 0;

      // File list background - always visible since we are always "ordered"
      ctx.fillStyle = `rgba(0,242,255,0.08)`;
      ctx.beginPath();
      ctx.roundRect(listX - 10, startY - 5, 180, fileData.length * lineHeight + 10, 6);
      ctx.fill();

      for (const f of files) {
        // Static position
        const y = f.y;

        // No icon anymore.
        // Identify hidden files by text color only.

        // Filename
        ctx.font = '600 10px monospace';
        ctx.textAlign = 'left';
        ctx.textBaseline = 'top';

        // Simpler color logic
        let textColor = f.hidden ? 'rgba(0,242,255,0.4)' : '#00f2ff';
        if (f.hidden && !showExt) textColor = 'rgba(170,170,170,0.5)'; // dim hidden if extensions not shown yet?

        ctx.fillStyle = textColor;

        // Show name, extension fades in
        if (showExt && f.ext) {
          const extAlpha = Math.min(extReveal * 1.5, 1);
          ctx.fillText(f.name, listX, y + 2); // Removed loose +20 offset for icon
          ctx.globalAlpha = extAlpha;
          ctx.fillStyle = '#00d4dd';
          ctx.fillText(f.ext, listX + ctx.measureText(f.name).width, y + 2);
          ctx.globalAlpha = 1;
        } else {
          ctx.fillText(f.name, listX, y + 2);
        }

        // Checkmark always visible for non-hidden files
        if (!f.hidden) {
          ctx.fillStyle = '#4ed99c';
          ctx.font = '11px sans-serif';
          ctx.textAlign = 'right';
          ctx.fillText('✓', listX + 160, y + 2);
        }

        // Subtle glow always on
        if (!f.hidden) {
          ctx.globalAlpha = 0.05;
          ctx.drawImage(cyanG, listX - 20, y - 5, 200, 30);
          ctx.globalAlpha = 1;
        }
      }

      raf = requestAnimationFrame(loop);
    };
    raf = requestAnimationFrame(loop);
    return () => cancelAnimationFrame(raf);
  }, []);

  return <div style={containerStyle}><canvas ref={ref} /></div>;
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Step 4 ▸ BrowserAnimation – YouTube Ad-Blocking Purification
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

export function BrowserAnimation() {
  const ref = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = ref.current;
    if (!canvas) return;
    const { ctx, w, h } = initCanvas(canvas);

    const CYCLE = 8; // Total loop time

    // Browser Window Dimensions
    const bW = w * 0.88;
    const bH = h * 0.78;
    const bX = (w - bW) / 2;
    const bY = (h - bH) / 2;
    const cx = w / 2;
    const adLabels = ['Ad 1 of 2', 'SKIP AD', 'BUY NOW', '0:15', 'CLICK', 'SUBSCRIBE', 'Ad', 'PROMO'];

    interface AdParticle {
      ox: number; oy: number; x: number; y: number;
      label: string; color: string; w: number; h: number;
    }

    // Grid-scatter instead of pure random placement: pure-random packed 8 ad
    // chips into a modest content area and they clustered/overlapped, reading
    // as "squeezed" rather than chaotic. A cell per ad with jitter guarantees spread.
    const GRID_COLS = 4;
    const GRID_ROWS = 2;
    const areaX = bX + 10;
    const areaY = bY + 50;
    const areaW = bW - 20;
    const areaH = bH - 60;
    const cellW = areaW / GRID_COLS;
    const cellH = areaH / GRID_ROWS;

    const ads: AdParticle[] = adLabels.map((label, i) => {
      const aw = rand(40, Math.min(70, cellW - 8));
      const ah = rand(20, Math.min(35, cellH - 8));
      const col = i % GRID_COLS;
      const row = Math.floor(i / GRID_COLS);
      const cellX = areaX + col * cellW;
      const cellY = areaY + row * cellH;
      const x = rand(cellX + 2, cellX + cellW - aw - 2);
      const y = rand(cellY + 2, cellY + cellH - ah - 2);
      const color = ['#ff5f57', '#febc2e', '#28c840', '#cc0000'][Math.floor(Math.random() * 4)];
      return { ox: x, oy: y, x, y, label, color, w: aw, h: ah };
    });

    let raf: number;
    let lastTs = 0;
    const startTime = performance.now() / 1000;

    const loop = (ts: number) => {
      if (!ctx || ts - lastTs < FRAME_MS) { raf = requestAnimationFrame(loop); return; }
      lastTs = ts;
      const now = performance.now() / 1000;
      const t = now - startTime;
      const phase = t % CYCLE;

      // Phases:
      // 0.0 - 2.5: Ads Active (Chaos)
      // 2.5 - 3.5: Sweep (Cleaning)
      // 3.5 - 7.0: Clean & Text
      // 7.0 - 8.0: Reset

      const isSweeping = phase >= 2.5 && phase < 3.5;
      const isClean = phase >= 3.5;
      const sweepProg = isSweeping ? (phase - 2.5) : (isClean ? 1 : 0);

      const sweepX = bX + sweepProg * bW;

      ctx.fillStyle = getBG();
      ctx.fillRect(0, 0, w, h);

      // ─── Draw Browser Chrome ───
      ctx.save();

      // Window Base
      ctx.shadowColor = 'rgba(0,0,0,0.5)';
      ctx.shadowBlur = 12;
      ctx.shadowOffsetY = 4;
      ctx.fillStyle = '#1e1e1e';
      ctx.beginPath();
      ctx.roundRect(bX, bY, bW, bH, 8);
      ctx.fill();
      ctx.shadowBlur = 0;
      ctx.shadowOffsetY = 0;

      ctx.strokeStyle = '#333';
      ctx.lineWidth = 1;
      ctx.stroke();

      // Title bar
      ctx.fillStyle = '#2d2d2d';
      ctx.beginPath();
      ctx.roundRect(bX, bY, bW, 28, [8, 8, 0, 0]);
      ctx.fill();

      // Traffic lights
      const dots = ['#ff5f57', '#febc2e', '#28c840'];
      dots.forEach((c, i) => {
        ctx.beginPath();
        ctx.arc(bX + 14 + i * 16, bY + 14, 4.5, 0, Math.PI * 2);
        ctx.fillStyle = c;
        ctx.fill();
      });

      // Address bar
      ctx.fillStyle = '#1e1e1e';
      ctx.beginPath();
      ctx.roundRect(bX + 70, bY + 6, bW - 80, 16, 4);
      ctx.fill();
      ctx.fillStyle = '#666';
      ctx.font = '10px monospace';
      ctx.textAlign = 'left';
      ctx.textBaseline = 'middle';
      const url = isClean ? 'https://youtube.com' : 'https://youtube.com/watch?v=...';
      ctx.fillText(url, bX + 78, bY + 14);
      if (isClean) {
        ctx.fillStyle = '#00f2ff';
        ctx.textAlign = 'right';
        ctx.fillText('🔒', bX + bW - 20, bY + 14);
      }

      // ─── Content Area ───
      const cY = bY + 28;
      const cH = bH - 28;

      // Clip content to window
      ctx.beginPath();
      ctx.rect(bX, cY, bW, cH);
      ctx.clip();

      // Video Player Background
      ctx.fillStyle = '#000';
      ctx.fillRect(bX, cY, bW, cH * 0.85);

      // Play Button / Center — hidden when ad-free overlay is showing
      if (!isClean) {
        ctx.fillStyle = '#333';
        ctx.beginPath();
        ctx.arc(cx, cY + cH * 0.42, 20, 0, Math.PI * 2);
        ctx.fill();
        ctx.fillStyle = '#fff';
        ctx.beginPath();
        ctx.moveTo(cx - 6, cY + cH * 0.42 - 8);
        ctx.lineTo(cx + 8, cY + cH * 0.42);
        ctx.lineTo(cx - 6, cY + cH * 0.42 + 8);
        ctx.fill();
      }

      // Video Controls Bar
      const bBarY = cY + cH * 0.85 - 6;
      ctx.fillStyle = '#222';
      ctx.fillRect(bX, bBarY, bW, 6);
      ctx.fillStyle = '#f00';
      ctx.fillRect(bX, bBarY, bW * 0.4, 6); // Progress

      // Yellow Ad Markers (only before sweep passes)
      if (!isClean) {
        ctx.fillStyle = '#fc0';
        [0.1, 0.25, 0.45, 0.6, 0.8, 0.95].forEach(p => {
          // If sweep passed this X, don't draw marker
          const mX = bX + bW * p;
          if (mX > sweepX) {
            ctx.fillRect(mX, bBarY, 3, 6);
          }
        });
      }

      // Sidebar Suggestions
      const sY = cY + cH * 0.85 + 8;
      ctx.fillStyle = '#222';
      for (let i = 0; i < 3; i++) {
        ctx.fillRect(bX + 10 + i * (bW / 3), sY, bW / 3 - 20, 10);
        ctx.fillStyle = '#333';
        ctx.fillRect(bX + 10 + i * (bW / 3), sY + 14, bW / 3 - 40, 4);
        ctx.fillStyle = '#222';
      }

      // ─── Ads ───
      if (!isClean) {
        for (const ad of ads) {
          // Check if swept
          if (isSweeping && ad.x < sweepX) continue;

          // Jitter
          const jx = ad.ox + Math.sin(t * 3 + ad.ox) * 2;
          const jy = ad.oy + Math.cos(t * 4 + ad.oy) * 2;

          ctx.save();
          ctx.translate(jx, jy);
          ctx.rotate(Math.sin(t * 2 + ad.ox) * 0.05);

          // Box
          ctx.fillStyle = '#eee';
          ctx.shadowColor = 'rgba(0,0,0,0.3)';
          ctx.shadowBlur = 4;
          ctx.fillRect(0, 0, ad.w, ad.h);
          ctx.shadowBlur = 0;

          // Header Color
          ctx.fillStyle = ad.color;
          ctx.fillRect(0, 0, 6, ad.h);

          // Label
          ctx.fillStyle = '#222';
          ctx.font = 'bold 9px sans-serif';
          ctx.textAlign = 'center';
          ctx.textBaseline = 'middle';
          ctx.fillText(ad.label, ad.w / 2 + 3, ad.h / 2);

          // Close btn
          ctx.fillStyle = '#ccc';
          ctx.fillRect(ad.w - 10, 0, 10, 10);
          ctx.fillStyle = '#555';
          ctx.font = '8px sans-serif';
          ctx.fillText('x', ad.w - 5, 5);

          ctx.restore();
        }
      }

      // ─── Sweep Line ───
      if (isSweeping) {
        const grad = ctx.createLinearGradient(sweepX - 40, 0, sweepX, 0);
        grad.addColorStop(0, 'rgba(0, 242, 255, 0)');
        grad.addColorStop(1, 'rgba(0, 242, 255, 0.4)');
        ctx.fillStyle = grad;
        ctx.fillRect(sweepX - 40, bY, 40, bH);

        ctx.beginPath();
        ctx.moveTo(sweepX, bY);
        ctx.lineTo(sweepX, bY + bH);
        ctx.strokeStyle = '#00f2ff';
        ctx.lineWidth = 1;
        ctx.stroke();

        // Sparkles at sweep edge
        for (let i = 0; i < 3; i++) {
          ctx.fillStyle = '#fff';
          const spY = bY + Math.random() * bH;
          ctx.fillRect(sweepX - Math.random() * 10, spY, 2, 2);
        }
      }

      // ─── Text "Ad Blocked" — drawn at video center, play button already hidden ───
      if (isClean) {
        const fadeT = Math.min((t - 3.5) * 2, 1);
        if (fadeT > 0) {
          ctx.save();
          // Anchor to play button position so text sits perfectly in the video area
          ctx.translate(cx, cY + cH * 0.42);
          const scale = 0.8 + fadeT * 0.2;
          ctx.scale(scale, scale);

          ctx.globalAlpha = fadeT;
          ctx.fillStyle = '#00f2ff';
          ctx.shadowColor = 'rgba(0, 242, 255, 0.5)';
          ctx.shadowBlur = 15;
          ctx.font = 'bold 22px sans-serif';
          ctx.textAlign = 'center';
          ctx.textBaseline = 'middle';
          ctx.fillText('ALL ADS BLOCKED', 0, -12);

          ctx.font = '11px monospace';
          ctx.fillStyle = 'rgba(255, 255, 255, 0.8)';
          ctx.shadowBlur = 0;
          ctx.fillText('EVEN YOUTUBE', 0, 14);

          ctx.restore();
        }
      }

      ctx.restore(); // End Browser Chrome Save

      raf = requestAnimationFrame(loop);
    };
    raf = requestAnimationFrame(loop);
    return () => cancelAnimationFrame(raf);
  }, []);

  return <div style={containerStyle}><canvas ref={ref} /></div>;
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Step 5 ▸ PrivacyShieldAnimation – Scanner Inside Laptop
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━




// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Step 5 ▸ PrivacyShieldAnimation – Typing Secret Data & Blur on Detect
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

export function PrivacyShieldAnimation() {
  const ref = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = ref.current;
    if (!canvas) return;
    const { ctx, w, h } = initCanvas(canvas);
    const cx = w / 2, cy = h / 2;
    const redG = makeGlow(255, 30, 30, 128);

    // Laptop dimensions
    const lapW = 240, lapH = 170;
    const scrX = cx - lapW / 2 + 12, scrY = cy - lapH / 2 + 8;
    const scrW = lapW - 24, scrH = lapH - 35;

    // "Document" content simulation
    const codeLines = [
      "CONFIDENTIAL MEMO",
      "-----------------",
      "SUBJECT: Project Chimera",
      "",
      "Financial Projections (Q4):",
      "  - Rev: $4.2B (+12%)",
      "  - Exp: $1.1B (-5%)",
      "",
      "Key Assets:",
      "  1. Quantum Algo X",
      "  2. Neural Net Y",
      "",
      "Password: ***********",
      "2FA Key: 883-192-441"
    ];

    const CYCLE = 8.0;

    const startTime = performance.now() / 1000;

    let raf: number;
    let lastTs = 0;
    const loop = (ts: number) => {
      if (!ctx || ts - lastTs < FRAME_MS) { raf = requestAnimationFrame(loop); return; }
      lastTs = ts;
      const now = performance.now() / 1000;
      const t = now - startTime;
      const phaseT = t % CYCLE;

      // Phases:
      // 0.0 - 4.0: Typing content (Blue sky)
      // 4.0 - 5.0: Camera Approach
      // 5.0 - 7.0: DETECTED (Blur + Alert)
      // 7.0 - 8.0: Reset/Fade

      let alert = false;
      let phoneY = h + 100;
      let blurAmount = 0;

      if (phaseT >= 4.0 && phaseT < 5.0) {
        // Approach
        const p = (phaseT - 4.0);
        const dp = 1 - Math.pow(1 - p, 3);
        phoneY = h + 60 - dp * 100;
      } else if (phaseT >= 5.0 && phaseT < 7.5) {
        // Detected
        phoneY = h - 40;
        alert = true;
        blurAmount = 8;
        if (phaseT > 7.0) {
          // fade out alert slightly?
        }
      } else if (phaseT >= 7.5) {
        // Retreat
        const p = (phaseT - 7.5) * 2; // quick retreat
        phoneY = h - 40 + p * 150;
      }

      // Typing progress (0 to 1 over first 3.5s)
      const typeProgress = Math.min(phaseT / 3.5, 1);
      const totalChars = codeLines.join('\n').length;
      const charsToShow = Math.floor(typeProgress * totalChars);

      ctx.save();
      ctx.fillStyle = getBG();
      ctx.fillRect(0, 0, w, h);

      const floatY = Math.sin(t * 0.8) * 3;
      ctx.translate(0, floatY);

      // ─── Laptop Shell ───
      // Screen bezel
      ctx.fillStyle = '#1a1e28';
      ctx.strokeStyle = alert ? 'rgba(255,50,50,0.4)' : 'rgba(80,100,130,0.4)';
      ctx.lineWidth = 2;
      ctx.beginPath();
      ctx.roundRect(cx - lapW / 2, cy - lapH / 2, lapW, lapH - 20, 8);
      ctx.fill();
      ctx.stroke();

      // Webcam dot
      const camY = cy - lapH / 2 + 5;
      ctx.beginPath();
      ctx.arc(cx, camY, 2.5, 0, Math.PI * 2);
      ctx.fillStyle = alert ? '#ff2222' : 'rgba(0,242,255,0.3)';
      ctx.fill();

      if (alert) {
        // flash red eye
        const flash = Math.abs(Math.sin(t * 10));
        ctx.beginPath();
        ctx.arc(cx, camY, 4 + flash * 2, 0, Math.PI * 2);
        ctx.strokeStyle = `rgba(255,0,0,${0.5 * flash})`;
        ctx.stroke();
      }

      // Laptop base
      const baseY = cy - lapH / 2 + lapH - 20;
      ctx.fillStyle = '#1a1e28';
      ctx.strokeStyle = alert ? 'rgba(255,50,50,0.3)' : 'rgba(80,100,130,0.3)';
      ctx.lineWidth = 1.5;
      ctx.beginPath();
      ctx.moveTo(cx - lapW / 2 - 8, baseY);
      ctx.lineTo(cx + lapW / 2 + 8, baseY);
      ctx.lineTo(cx + lapW / 2 + 16, baseY + 18);
      ctx.lineTo(cx - lapW / 2 - 16, baseY + 18);
      ctx.closePath();
      ctx.fill();
      ctx.stroke();

      // Screen Content Area
      ctx.save();
      ctx.beginPath();
      ctx.rect(scrX, scrY, scrW, scrH);
      ctx.clip();

      // Apply blur if detected
      if (blurAmount > 0) {
        ctx.filter = `blur(${blurAmount}px)`;
      }

      // White "Word Doc" background
      ctx.fillStyle = '#f5f5f5';
      ctx.fillRect(scrX, scrY, scrW, scrH);

      // Typing Text
      ctx.font = '10px "Courier New", monospace';
      ctx.fillStyle = '#000';
      ctx.textBaseline = 'top';

      let charCount = 0;
      let cursorY = scrY + 10;
      const lineHeight = 12;

      for (const line of codeLines) {
        const remaining = charsToShow - charCount;
        if (remaining <= 0) break;

        let lineText = line;
        if (line.length > remaining) {
          lineText = line.substring(0, remaining);
        }

        ctx.fillText(lineText, scrX + 10, cursorY);

        charCount += line.length; // Count full line logic for simplicity
        cursorY += lineHeight;
        // Simple cursor pos hack
        if (line.length <= remaining) {
          // full line drawn
        } else {
          // partial line
          // const width = ctx.measureText(lineText).width;
          // cursorX = scrX + 10 + width;
        }
      }

      // Draw Cursor if typing
      if (charsToShow < totalChars && !alert) {
        // Re-calculate cursor position precisely? 
        // For simplicity, just blink at the end of the last drawn text
        // or just at the bottom
        const blink = Math.sin(t * 15) > 0;
        if (blink) {
          ctx.fillStyle = '#000';
          ctx.fillRect(scrX + 10, cursorY, 6, 10);
        }
      }

      ctx.restore(); // End clip (filters apply here)

      // Alert Overlay (Must be SHARP, so after clip restore)
      if (alert) {
        const alertY = cy;
        ctx.fillStyle = 'rgba(10,0,0,0.85)';
        ctx.beginPath();
        ctx.roundRect(cx - 70, alertY - 12, 140, 24, 4);
        ctx.fill();
        ctx.strokeStyle = '#ff2222';
        ctx.lineWidth = 1;
        ctx.stroke();

        ctx.fillStyle = '#ff2222';
        ctx.font = 'bold 11px sans-serif';
        ctx.textAlign = 'center';
        ctx.textBaseline = 'middle';
        ctx.fillText('⚠ PHONE DETECTED ⚠', cx, alertY);

        // Red glow
        ctx.globalAlpha = 0.3;
        ctx.drawImage(redG, cx - 100, cy - 100, 200, 200);
        ctx.globalAlpha = 1;
      }

      ctx.restore(); // End float

      // ─── Intruder Phone ───
      if (phoneY < h + 50) {
        const phW = 40, phH = 70;
        const phX = cx - phW / 2;
        ctx.shadowBlur = 10;
        ctx.shadowColor = 'rgba(0,0,0,0.5)';
        ctx.fillStyle = '#111';
        ctx.beginPath();
        ctx.roundRect(phX, phoneY, phW, phH, 6);
        ctx.fill();
        ctx.strokeStyle = '#444';
        ctx.lineWidth = 1.5;
        ctx.stroke();
        ctx.shadowBlur = 0;

        // Lens
        ctx.beginPath();
        ctx.arc(phX + phW / 2, phoneY + 15, 6, 0, Math.PI * 2);
        ctx.fillStyle = '#333';
        ctx.fill();
        ctx.strokeStyle = '#666';
        ctx.lineWidth = 1;
        ctx.stroke();
        ctx.beginPath();
        ctx.arc(phX + phW / 2, phoneY + 15, 2, 0, Math.PI * 2);
        ctx.fillStyle = '#0ff';
        ctx.fill();

        // Rec dot
        if (Math.sin(t * 10) > 0) {
          ctx.fillStyle = '#f00';
          ctx.beginPath();
          ctx.arc(phX + phW / 2 + 10, phoneY + 15, 1.5, 0, Math.PI * 2);
          ctx.fill();
        }
      }

      raf = requestAnimationFrame(loop);
    };
    raf = requestAnimationFrame(loop);
    return () => cancelAnimationFrame(raf);
  }, []);

  return <div style={containerStyle}><canvas ref={ref} /></div>;
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Step 6 ▸ FirewallAnimation – Dynamic Hex-Grid Wall
//   blockedIds = set of IDs that are BLOCKED (hit wall).
//   Everything else passes through.
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

export interface FirewallAnimationProps {
  blockedIds: Set<string>;
}

interface FwAppDef {
  id: string; name: string; color: string; logo: string;
}

const FW_APPS: FwAppDef[] = [
  { id: 'telemetry-blocklist', name: 'Telemetry',   color: '#fd79a8', logo: companyLogos['microsoft-logo.svg'] },
  { id: 'ai-sites',           name: 'AI Services',  color: '#a29bfe', logo: companyLogos['google-logo.svg'] },
  { id: 'adobe',              name: 'Adobe',         color: '#ff3344', logo: companyLogos['adobe-logo.png'] },
  { id: 'autodesk',           name: 'Autodesk',      color: '#ff9f43', logo: software['autocad.png'] },
  { id: 'corel',              name: 'Corel',         color: '#6c5ce7', logo: software['coreldraw.png'] },
  { id: 'piracy-torrent',     name: 'Torrents',      color: '#e17055', logo: software['piratebay.png'] },
  { id: 'glasswire',          name: 'GlassWire',     color: '#00b894', logo: software['glasswire.png'] },
  { id: 'lightburn',          name: 'LightBurn',     color: '#fdcb6e', logo: software['lightburn.png'] },
  { id: 'cloud-upload',       name: 'Cloud Upload',  color: '#74b9ff', logo: saas['gdrive.svg'] },
];

export function FirewallAnimation({ blockedIds }: FirewallAnimationProps) {
  const ref = useRef<HTMLCanvasElement>(null);
  const blockedRef = useRef(blockedIds);
  blockedRef.current = blockedIds;

  useEffect(() => {
    const canvas = ref.current;
    if (!canvas) return;
    // Ambient, non-interactive animation: cap backing resolution at 1.25x DPR
    // (vs the shared 2x) to slash fragment/fill cost on the large internet zone.
    const { ctx, w, h } = initCanvas(canvas, undefined, 1.25);

    // Ambient frame cap (~20 fps). This step is background atmosphere, not
    // interactive, so a lower cap is imperceptible but meaningfully cuts the
    // per-second GPU/CPU work vs the shared 30 fps.
    const FW_FRAME_MS = 1000 / 20;

    const wallX = w * 0.5;
    const hexR = 14;

    interface HexCell { x: number; y: number; flash: number; }
    const hexCells: HexCell[] = [];
    const hexH = hexR * Math.sqrt(3);
    const rowCount = Math.ceil(h / hexH) + 1;
    const colCount = 3;
    for (let r = 0; r < rowCount; r++) {
      for (let c = 0; c < colCount; c++) {
        const hx = wallX + c * hexR * 1.6;
        const hy = r * hexH + (c % 2 ? hexH / 2 : 0) - hexH;
        hexCells.push({ x: hx, y: hy, flash: 0 });
      }
    }

    // Preload logos
    const logoImgs: Record<string, HTMLImageElement> = {};
    for (const app of FW_APPS) {
      if (app.logo) {
        const img = new Image();
        img.src = app.logo;
        logoImgs[app.id] = img;
      }
    }

    // Position apps vertically evenly
    const appSpacing = h / (FW_APPS.length + 1);
    const appPositions = FW_APPS.map((a, i) => ({
      ...a, y: appSpacing * (i + 1),
    }));

    interface FwPacket {
      x: number; y: number; speed: number; color: string;
      blocked: boolean; passed: boolean;
      life: number; alive: boolean; sparks: Spark[];
      appId: string;
    }

    const packets: FwPacket[] = [];
    let nextSpawn = 0;

    // Ambient drift motes for the Internet zone — gives the right side a
    // sense of live network traffic even when no packets are crossing.
    // Initialised lazily on the first frame so we know the zone bounds.
    interface DriftMote { x: number; y: number; vx: number; vy: number; r: number; tw: number; }
    const motes: DriftMote[] = [];
    let motesInit = false;

    // ── One-time hoisted statics ───────────────────────────────────────
    // The internet zone geometry is fixed for the canvas lifetime, so the
    // backdrop gradient + depth-glow sprites + node graph are built ONCE
    // here instead of re-created on every frame (createLinear/RadialGradient
    // per frame is a major GPU/CPU cost driver).
    const zoneLeft = wallX + (hexR * 1.6 * colCount) + 8;
    const zoneW = w - zoneLeft;

    // Static backdrop gradient (left→right). Position-fixed, so build once.
    const zoneGrad = ctx ? ctx.createLinearGradient(zoneLeft, 0, w, 0) : null;
    if (zoneGrad) {
      zoneGrad.addColorStop(0.00, 'rgba(6,18,30,0.00)');
      zoneGrad.addColorStop(0.12, 'rgba(4,60,90,0.06)');
      zoneGrad.addColorStop(0.40, 'rgba(0,120,170,0.12)');
      zoneGrad.addColorStop(0.72, 'rgba(0,80,130,0.18)');
      zoneGrad.addColorStop(1.00, 'rgba(0,38,66,0.26)');
    }

    // Pre-rendered depth-glow sprite (replaces two per-frame radial gradients).
    // Blitted additively at two fixed spots below — sprite blit is far cheaper
    // than rebuilding radial gradients every frame.
    const cyanGlow = makeGlow(0, 210, 255, 128);

    // Internet nodes + precomputed visible connection pairs. Node positions
    // never move, so the O(n²) distance test is done once at setup rather
    // than every frame.
    const iNodes = [
      { px: 0.12, py: 0.18 }, { px: 0.55, py: 0.10 }, { px: 0.82, py: 0.25 },
      { px: 0.25, py: 0.50 }, { px: 0.70, py: 0.45 }, { px: 0.45, py: 0.75 },
      { px: 0.88, py: 0.68 }, { px: 0.15, py: 0.82 }, { px: 0.60, py: 0.90 },
    ].map(n => ({ ...n, x: zoneLeft + n.px * zoneW, y: n.py * h }));
    const nodeLinks: { ax: number; ay: number; bx: number; by: number }[] = [];
    for (let ni = 0; ni < iNodes.length; ni++) {
      for (let nj = ni + 1; nj < iNodes.length; nj++) {
        const a = iNodes[ni], b = iNodes[nj];
        if (Math.hypot(b.x - a.x, b.y - a.y) < zoneW * 0.55) {
          nodeLinks.push({ ax: a.x, ay: a.y, bx: b.x, by: b.y });
        }
      }
    }

    const startTime = performance.now() / 1000;
    let raf: number;
    let lastTs = 0;

    const loop = (ts: number) => {
      if (!ctx || ts - lastTs < FW_FRAME_MS) { raf = requestAnimationFrame(loop); return; }
      lastTs = ts;
      const now = performance.now() / 1000;
      const t = now - startTime;
      // Per-frame fixed step kept at 1/60 (unchanged) so packet speeds and
      // spark/flash decay stay visually identical; only the paint cadence is
      // lowered to ~20 fps via FW_FRAME_MS above.
      const dt = 1 / 60;
      const currentBlocked = blockedRef.current;
      const p0 = wizardPalette();

      ctx.fillStyle = getBG();
      ctx.fillRect(0, 0, w, h);

      // Spawn packets from multiple apps simultaneously. Bumped up vs the
      // old defaults to make the wall feel under genuine load: more apps
      // sending at once, more packets per burst, shorter pause between
      // bursts. Combined with the ambient drift particles below, the
      // internet side reads as "alive" rather than mostly empty.
      if (t > nextSpawn) {
        const batchSize = Math.min(2 + Math.floor(Math.random() * 2), appPositions.length);
        const shuffled = [...appPositions].sort(() => Math.random() - 0.5);
        for (let b = 0; b < batchSize; b++) {
          const appDef = shuffled[b];
          const burst = 3 + Math.floor(Math.random() * 2);
          for (let i = 0; i < burst; i++) {
            packets.push({
              x: 80 + i * 12,
              y: appDef.y + rand(-6, 6),
              speed: rand(160, 280),
              color: appDef.color,
              blocked: false, passed: false,
              life: 1, alive: true, sparks: [],
              appId: appDef.id,
            });
          }
        }
        nextSpawn = t + rand(0.35, 0.6);
      }

      // Update packets
      for (const p of packets) {
        if (!p.alive) continue;
        const isBlocked = currentBlocked.has(p.appId);
        if (!p.blocked && !p.passed) {
          p.x += p.speed * dt;
          if (p.x >= wallX - 5) {
            if (isBlocked) {
              // HIT THE WALL
              p.blocked = true;
              p.life = 1;
              let minD = 999; let bestHex: HexCell | null = null;
              for (const hc of hexCells) {
                const d = Math.abs(hc.y - p.y);
                if (d < minD) { minD = d; bestHex = hc; }
              }
              if (bestHex) bestHex.flash = 1;
              // Impact burst — kept punchy but trimmed to 6 sparks (was 14)
              // to cut the per-blocked-packet allocation + per-frame draw cost.
              for (let k = 0; k < 6; k++) {
                const a = rand(-Math.PI * 0.85, Math.PI * 0.85);
                const sp = rand(80, 220);
                p.sparks.push({
                  x: p.x, y: p.y,
                  vx: -Math.abs(Math.cos(a)) * sp,
                  vy: Math.sin(a) * sp * 0.8,
                  life: 1,
                });
              }
            } else {
              // PASS THROUGH — allow to continue to cloud
              p.passed = true;
            }
          }
        } else if (p.passed) {
          p.x += p.speed * dt;
          if (p.x > w + 20) p.alive = false;
        } else {
          // blocked, decaying
          p.life -= dt * 3;
          if (p.life <= 0) p.alive = false;
        }
        for (const s of p.sparks) { s.x += s.vx * dt; s.y += s.vy * dt; s.life -= dt * 3.5; }
        p.sparks = p.sparks.filter(s => s.life > 0);
      }

      for (const hc of hexCells) { if (hc.flash > 0) hc.flash -= dt * 2.5; }

      // Draw app labels (left side)
      const LOGO_SZ = 16;
      ctx.textBaseline = 'middle';
      for (const ap of appPositions) {
        const isBlk = currentBlocked.has(ap.id);
        const img = logoImgs[ap.id];
        const hasLogo = img && img.complete && img.naturalWidth > 0;
        const labelX = hasLogo ? 12 + LOGO_SZ + 5 : 14;

        if (hasLogo) {
          ctx.globalAlpha = 0.85;
          ctx.drawImage(img, 12, ap.y - LOGO_SZ / 2, LOGO_SZ, LOGO_SZ);
          ctx.globalAlpha = 1;
        }

        ctx.font = '700 14px monospace';
        ctx.textAlign = 'left';
        ctx.fillStyle = ap.color;
        ctx.fillText(ap.name, labelX, ap.y);

        // Status indicator
        ctx.font = '700 11px monospace';
        ctx.fillStyle = isBlk ? 'rgba(255,68,68,0.85)' : 'rgba(76,217,100,0.85)';
        ctx.fillText(isBlk ? 'BLOCKED' : 'ALLOWED', labelX, ap.y + 16);

        // Dashed trail
        const trailStart = labelX + 70;
        const trailA = 0.08 * p0.alphaBoost * 2;
        ctx.strokeStyle = isBlk ? `rgba(${p0.danger},${trailA})` : `rgba(${p0.success},${trailA})`;
        ctx.lineWidth = 0.6 * p0.strokeBoost;
        ctx.setLineDash([3, 4]);
        ctx.beginPath();
        ctx.moveTo(trailStart, ap.y);
        ctx.lineTo(wallX - 10, ap.y);
        ctx.stroke();
        ctx.setLineDash([]);
      }

      // Internet zone (right side) – full atmospheric backdrop.
      // zoneLeft / zoneW / zoneGrad are hoisted one-time statics (geometry is
      // fixed for the canvas lifetime).
      ctx.fillStyle = zoneGrad ?? `rgba(0,80,130,0.12)`;
      ctx.fillRect(zoneLeft, 0, zoneW, h);

      // Depth glows: blit the pre-rendered radial sprite at two fixed spots
      // (replaces two per-frame createRadialGradient calls). Sprite blitting
      // is dramatically cheaper than rebuilding gradients each frame.
      ctx.save();
      ctx.globalCompositeOperation = 'lighter';
      ctx.globalAlpha = 0.5;
      const g1s = zoneW * 1.2;
      ctx.drawImage(cyanGlow, zoneLeft + zoneW * 0.68 - g1s / 2, h * 0.32 - g1s / 2, g1s, g1s);
      ctx.globalAlpha = 0.32;
      const g2s = zoneW * 0.96;
      ctx.drawImage(cyanGlow, zoneLeft + zoneW * 0.28 - g2s / 2, h * 0.76 - g2s / 2, g2s, g2s);
      ctx.globalAlpha = 1;
      ctx.restore();

      // Subtle top/bottom border lines
      ctx.strokeStyle = `rgba(${p0.accent},${0.10 * p0.alphaBoost})`;
      ctx.lineWidth = 1 * p0.strokeBoost;
      ctx.beginPath(); ctx.moveTo(zoneLeft, 1); ctx.lineTo(w, 1); ctx.stroke();
      ctx.beginPath(); ctx.moveTo(zoneLeft, h - 1); ctx.lineTo(w, h - 1); ctx.stroke();

      // Ambient drifting motes — populated once we know the zone width.
      // They wrap around at the right edge so the field stays full at all
      // times without the cost of constantly spawning/despawning.
      if (!motesInit) {
        const N = 18;
        for (let i = 0; i < N; i++) {
          motes.push({
            x: zoneLeft + Math.random() * zoneW,
            y: Math.random() * h,
            vx: rand(8, 28),
            vy: rand(-6, 6),
            r: rand(0.6, 1.8),
            tw: Math.random() * Math.PI * 2,
          });
        }
        motesInit = true;
      }
      ctx.save();
      ctx.globalCompositeOperation = 'lighter';
      for (const m of motes) {
        m.x += m.vx * dt;
        m.y += m.vy * dt;
        m.tw += dt * 3;
        if (m.x > w + 4) m.x = zoneLeft - 4;
        if (m.y < -4) m.y = h + 4;
        if (m.y > h + 4) m.y = -4;
        const a = (0.18 + 0.28 * (0.5 + 0.5 * Math.sin(m.tw))) * p0.alphaBoost;
        ctx.beginPath();
        ctx.arc(m.x, m.y, m.r, 0, Math.PI * 2);
        ctx.fillStyle = `rgba(${p0.accent},${Math.min(1, a)})`;
        ctx.fill();
      }
      ctx.restore();

      // Thin connecting lines between nearby nodes. Visible pairs are
      // precomputed once (nodeLinks); coords are cached, so no per-frame
      // hypot/O(n²). Batched into a single path+stroke with one shimmer alpha.
      ctx.strokeStyle = `rgba(${p0.accentSoft},${(0.07 + 0.03 * Math.sin(t)) * p0.alphaBoost})`;
      ctx.lineWidth = 0.7 * p0.strokeBoost;
      ctx.beginPath();
      for (const lk of nodeLinks) {
        ctx.moveTo(lk.ax, lk.ay); ctx.lineTo(lk.bx, lk.by);
      }
      ctx.stroke();

      // Floating internet nodes (positions hoisted into iNodes).
      for (const nd of iNodes) {
        const pulse = 0.35 + 0.25 * Math.sin(t * 1.3 + nd.px * 8);
        ctx.beginPath();
        ctx.arc(nd.x, nd.y, 3.5, 0, Math.PI * 2);
        ctx.fillStyle = `rgba(${p0.accentSoft},${Math.min(1, pulse * p0.alphaBoost)})`;
        ctx.fill();
        // Outer glow ring
        ctx.beginPath();
        ctx.arc(nd.x, nd.y, 6 + 2 * Math.sin(t * 1.3 + nd.px * 8), 0, Math.PI * 2);
        ctx.strokeStyle = `rgba(${p0.accentSoft},${Math.min(1, pulse * 0.4 * p0.alphaBoost)})`;
        ctx.lineWidth = 1 * p0.strokeBoost;
        ctx.stroke();
      }

      // INTERNET label
      ctx.font = '800 22px monospace';
      ctx.fillStyle = `rgba(${p0.accent},${Math.min(1, (0.55 + 0.15 * Math.sin(t * 0.8)) * p0.alphaBoost)})`;
      ctx.textAlign = 'center';
      ctx.textBaseline = 'middle';
      ctx.fillText('INTERNET', zoneLeft + zoneW / 2, h / 2);
      // Sub-label
      ctx.font = '500 10px monospace';
      ctx.fillStyle = `rgba(${p0.accentSoft},${Math.min(1, 0.30 * p0.alphaBoost)})`;
      ctx.fillText('PUBLIC NETWORK', zoneLeft + zoneW / 2, h / 2 + 20);

      // Hex wall
      for (const hc of hexCells) {
        const flash = Math.max(hc.flash, 0);
        const base = 0.12;
        const a = Math.min(1, (base + flash * 0.7) * (flash > 0.3 ? 1 : p0.alphaBoost));
        const c = flash > 0.3
          ? `rgba(${p0.danger},${a})`
          : `rgba(${p0.accent},${a})`;
        hexPath(ctx, hc.x, hc.y, hexR - 1);
        ctx.strokeStyle = c;
        ctx.lineWidth = (flash > 0.3 ? 2 : 1) * p0.strokeBoost;
        ctx.stroke();
        if (flash > 0.3) {
          ctx.globalAlpha = flash * 0.15;
          hexPath(ctx, hc.x, hc.y, hexR - 1);
          ctx.fillStyle = `rgb(${p0.danger})`;
          ctx.fill();
          ctx.globalAlpha = 1;
        }
      }

      // Firewall label
      ctx.save();
      ctx.translate(wallX + 10, h / 2);
      ctx.rotate(-Math.PI / 2);
      ctx.font = '700 8px monospace';
      ctx.fillStyle = `rgba(${p0.accent},${Math.min(1, 0.3 * p0.alphaBoost)})`;
      ctx.textAlign = 'center';
      ctx.textBaseline = 'middle';
      ctx.fillText('F I R E W A L L', 0, 0);
      ctx.restore();

      // Draw packets
      for (const p of packets) {
        if (!p.alive) continue;
        if (p.blocked) {
          ctx.globalAlpha = p.life;
          ctx.font = '12px sans-serif';
          ctx.textAlign = 'center';
          ctx.textBaseline = 'middle';
          ctx.fillText('❌', p.x - 5, p.y);
          ctx.globalAlpha = 1;
        } else {
          // Active packet (flying or passed through).
          // Trail: a faint short stroke instead of a per-packet
          // createLinearGradient (which was rebuilt for every packet every
          // frame — a dominant allocation cost). The head dot keeps the comet
          // read; the trail is a single low-alpha flat stroke.
          ctx.globalAlpha = 0.4;
          ctx.strokeStyle = p.color;
          ctx.lineWidth = 2;
          ctx.beginPath();
          ctx.moveTo(p.x - 12, p.y);
          ctx.lineTo(p.x, p.y);
          ctx.stroke();
          ctx.globalAlpha = 1;
          ctx.beginPath();
          ctx.arc(p.x, p.y, 3, 0, Math.PI * 2);
          ctx.fillStyle = p.color;
          ctx.fill();

          // Checkmark for passed-through packets near the cloud
          if (p.passed && p.x > wallX + 30) {
            ctx.globalAlpha = Math.min(1, (p.x - wallX - 30) / 60);
            ctx.font = '10px sans-serif';
            ctx.textAlign = 'center';
            ctx.textBaseline = 'middle';
            ctx.fillStyle = '#4ed99c';
            ctx.fillText('✓', p.x, p.y - 8);
            ctx.globalAlpha = 1;
          }
        }
        for (const s of p.sparks) {
          ctx.globalAlpha = s.life;
          ctx.beginPath();
          ctx.arc(s.x, s.y, 1.2, 0, Math.PI * 2);
          ctx.fillStyle = p.color;
          ctx.fill();
        }
        ctx.globalAlpha = 1;
      }

      // Cleanup
      for (let i = packets.length - 1; i >= 0; i--)
        if (!packets[i].alive && packets[i].sparks.length === 0) packets.splice(i, 1);

      raf = requestAnimationFrame(loop);
    };
    raf = requestAnimationFrame(loop);
    return () => cancelAnimationFrame(raf);
  }, []);

  return <div style={containerStyle}><canvas ref={ref} /></div>;
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// RadarScanAnimation – Full-System Threat Radar with Category Sectors
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

export interface ScanFinding {
  id: string;
  category: 'privacy' | 'performance' | 'annoyance' | 'engines' | 'updates';
  label: string;
  impact: string;
  severity: 'critical' | 'warning' | 'info';
  /** If true, this fix is safe for all users and won't break anything */
  safeDefault?: boolean;
  /** True when this finding means ideal.* differs from current.*. */
  drift?: boolean;
  /** Desired checked state to re-apply when fixing drift. */
  targetChecked?: boolean;
}

export interface ScanReport {
  findings: ScanFinding[];
  cpuUsage: number;
  ramUsage: number;
  diskReclaimableGb: number;
}

export interface RadarScanAnimationProps {
  phase: 'idle' | 'scanning' | 'complete';
  report: ScanReport | null;
  /** Render on transparent bg (for dashboard). Defaults false = opaque #080c12. */
  transparent?: boolean;
  /** Canvas height override. Defaults to 440. */
  height?: number;
  /**
   * Override the auto-detected optimal state. When provided, gates the
   * "SYSTEM OPTIMAL" display on additional caller-known signals (e.g. no
   * pending app updates) on top of the no-findings check.
   */
  optimal?: boolean;
  /**
   * Pending app update count to surface inside the radar HUD. The action
   * button itself stays uncluttered ("Update All Apps") — the actual count
   * lives here so the radar reads like a status display.
   */
  pendingAppUpdates?: number;
}

/* ── internal blip ── */
interface _RBlip {
  angle: number;
  radius: number;
  label: string;
  color: string;
  fadeIn: number;
  locked: boolean;
  pulseOff: number;
  revealAt: number; // seconds offset from reveal start
}

const _CAT_CLR: Record<string, string> = {
  privacy: '#ff2d55',
  performance: '#ff9500',
  annoyance: '#5ac8fa',
};

function _buildBlips(findings: ScanFinding[]): _RBlip[] {
  // Sort: safe/recommended findings first (inner ring), advanced last (outer ring)
  const sorted = [...findings].sort((a, b) => {
    if (a.safeDefault && !b.safeDefault) return -1;
    if (!a.safeDefault && b.safeDefault) return 1;
    return 0;
  });

  const n = sorted.length;
  return sorted.map((f, i) => {
    // Spread evenly around the full circle — no fixed category sectors
    const angle = -Math.PI / 2 + (i / Math.max(n, 1)) * Math.PI * 2;
    const r = f.safeDefault
      ? 0.35 + (i % 3) * 0.06
      : f.severity === 'critical' ? 0.68 + (i % 3) * 0.06
      : f.severity === 'warning'  ? 0.52 + (i % 3) * 0.08
      : 0.28 + (i % 2) * 0.1;
    const color = f.safeDefault ? '#22c55e' : (_CAT_CLR[f.category] ?? _CAT_CLR.privacy);
    return {
      angle, radius: clamp(r, 0.25, 0.88), label: f.label,
      color, fadeIn: 0, locked: false,
      pulseOff: Math.random() * Math.PI * 2, revealAt: i * 0.12,
    };
  });
}

export function RadarScanAnimation({ phase, report, transparent = false, height = 440, optimal, pendingAppUpdates }: RadarScanAnimationProps) {
  const ref = useRef<HTMLCanvasElement>(null);
  const phaseRef = useRef(phase);
  const reportRef = useRef(report);
  const transparentRef = useRef(transparent);
  const optimalRef = useRef(optimal);
  const pendingAppUpdatesRef = useRef(pendingAppUpdates);
  phaseRef.current = phase;
  reportRef.current = report;
  transparentRef.current = transparent;
  optimalRef.current = optimal;
  pendingAppUpdatesRef.current = pendingAppUpdates;

  useEffect(() => {
    const canvas = ref.current;
    if (!canvas) return;
    let { ctx, w, h } = initCanvas(canvas, height);
    let cx = w / 2, cy = h / 2;
    let maxR = Math.min(cx, cy) * 0.82;

    // Re-init if container was 0-width at mount time (layout race)
    const ro = new ResizeObserver(() => {
      const newW = canvas.parentElement?.clientWidth ?? 0;
      if (newW > 0 && Math.abs(newW - w) > 4) {
        ({ ctx, w, h } = initCanvas(canvas, height));
        cx = w / 2; cy = h / 2;
        maxR = Math.min(cx, cy) * 0.82;
      }
    });
    if (canvas.parentElement) ro.observe(canvas.parentElement);

    /* Glow sprites */
    const cyanG = makeGlow(0, 242, 255, 96);
    const greenG = makeGlow(78, 217, 156, 96); // Green glow for optimal state
    const redG  = makeGlow(255, 45, 85, 64);
    const ambG  = makeGlow(255, 149, 0, 64);
    const bluG  = makeGlow(90, 200, 250, 64);
    const _glow = (c: string) => c === '#ff2d55' ? redG : c === '#ff9500' ? ambG : bluG;

    let blips: _RBlip[] = [];
    let revealT = 0;
    let built = false;
    let lastRpt: ScanReport | null = null;
    const t0 = performance.now() / 1000;
    const SPEED = 0.45; // Snappier sweep

    /* Spark particles */
    interface Sp { x: number; y: number; vx: number; vy: number; life: number; max: number }
    const sparks: Sp[] = [];

    let raf: number;
    let lastTs = 0;

    const loop = (ts: number) => {
      if (!ctx || ts - lastTs < (1000 / 30)) { raf = requestAnimationFrame(loop); return; }
      lastTs = ts;
      const now = performance.now() / 1000;
      const t = now - t0;
      const dt = 1 / 60;
      const ph = phaseRef.current;
      const rpt = reportRef.current;
      const noFindings = ph === 'complete' && rpt && rpt.findings.length === 0;
      // When the caller passes an explicit `optimal` prop, gate on it as well
      // (e.g. dashboard hides SYSTEM OPTIMAL while app updates are pending).
      const isOptimal = noFindings && (optimalRef.current === undefined || optimalRef.current === true);
      const accentColor = isOptimal ? '#4ed99c' : '#00f2ff';
      const accentGlow = isOptimal ? greenG : cyanG;

      /* Build blips once when report arrives, but preserve fadeIn if possible */
      if (ph === 'complete' && rpt && (!built || rpt !== lastRpt)) {
        const nextBlips = _buildBlips(rpt.findings);
        // Preserve fadeIn state for existing blips to prevent flickering
        if (blips.length > 0) {
          nextBlips.forEach(nb => {
            const eb = blips.find(b => b.label === nb.label);
            if (eb) nb.fadeIn = eb.fadeIn;
          });
        }
        blips = nextBlips;
        if (!built) revealT = t;
        built = true;
        lastRpt = rpt;
      }

      /* Theme detection */
      const isLight = document.documentElement.classList.contains('light');
      const bgColor = transparentRef.current ? 'transparent' : (isLight ? '#f0f4f8' : BG);
      const gridBase = isOptimal
        ? (isLight ? 'rgba(34,197,94,' : 'rgba(78,217,156,')
        : (isLight ? 'rgba(14,165,233,' : 'rgba(0,242,255,');

      if (transparentRef.current) {
        ctx.clearRect(0, 0, w, h);
      } else {
        ctx.fillStyle = bgColor;
        ctx.fillRect(0, 0, w, h);
      }

      /* ── Grid rings ── */
      for (let i = 1; i <= 5; i++) {
        const rr = maxR * (i / 5);
        ctx.beginPath();
        ctx.arc(cx, cy, rr, 0, Math.PI * 2);
        const ringAlpha = i === 5 ? (isLight ? 0.35 : 0.18) : (isLight ? 0.15 : 0.06);
        ctx.strokeStyle = `${gridBase}${ringAlpha})`;
        ctx.lineWidth = i === 5 ? 1.5 : 0.8;
        ctx.stroke();
      }

      /* ── Cross-hairs ── */
      ctx.strokeStyle = isOptimal ? 'rgba(78,217,156,0.06)' : 'rgba(0,242,255,0.04)';
      ctx.lineWidth = 0.5;
      ctx.setLineDash([3, 10]);
      ctx.beginPath(); ctx.moveTo(cx - maxR, cy); ctx.lineTo(cx + maxR, cy); ctx.stroke();
      ctx.beginPath(); ctx.moveTo(cx, cy - maxR); ctx.lineTo(cx, cy + maxR); ctx.stroke();
      ctx.setLineDash([]);

      /* ── Sweep beam ── */
      if (ph !== 'idle') {
        const spd = ph === 'complete' ? SPEED * 0.25 : SPEED;
        const sa = (t * spd * Math.PI * 2) % (Math.PI * 2);
        const trail = Math.PI * 0.7;

        /* Gradient trail wedge */
        ctx.save();
        ctx.beginPath();
        ctx.moveTo(cx, cy);
        ctx.arc(cx, cy, maxR, sa - trail, sa, false);
        ctx.closePath();
        const sg = ctx.createRadialGradient(cx, cy, 0, cx, cy, maxR);
        sg.addColorStop(0, isOptimal ? 'rgba(78,217,156,0.15)' : 'rgba(0,242,255,0.12)');
        sg.addColorStop(0.5, isOptimal ? 'rgba(78,217,156,0.06)' : 'rgba(0,242,255,0.04)');
        sg.addColorStop(1, 'rgba(0,242,255,0.00)');
        ctx.fillStyle = sg;
        ctx.globalAlpha = 0.65;
        ctx.fill();
        ctx.globalAlpha = 1;
        ctx.restore();

        /* Leading edge */
        const ex = cx + Math.cos(sa) * maxR;
        const ey = cy + Math.sin(sa) * maxR;
        ctx.save();
        ctx.beginPath();
        ctx.moveTo(cx, cy);
        ctx.lineTo(ex, ey);
        ctx.strokeStyle = isOptimal ? 'rgba(78,217,156,0.85)' : 'rgba(0,242,255,0.75)';
        ctx.lineWidth = 1.5;
        ctx.shadowColor = accentColor;
        ctx.shadowBlur = 10;
        ctx.stroke();
        ctx.shadowBlur = 0;
        ctx.restore();

        /* Sparks at sweep tip */
        if (ph === 'scanning' && Math.random() > 0.55) {
          sparks.push({
            x: ex, y: ey,
            vx: (Math.random() - 0.5) * 70,
            vy: (Math.random() - 0.5) * 70,
            life: 0.5 + Math.random() * 0.5,
            max: 0.5 + Math.random() * 0.5,
          });
        }
      }

      /* ── Update & draw sparks ── */
      for (let i = sparks.length - 1; i >= 0; i--) {
        const s = sparks[i];
        s.x += s.vx * dt;
        s.y += s.vy * dt;
        s.life -= dt;
        if (s.life <= 0) { sparks.splice(i, 1); continue; }
        ctx.globalAlpha = (s.life / s.max) * 0.55;
        ctx.beginPath();
        ctx.arc(s.x, s.y, 1.5, 0, Math.PI * 2);
        ctx.fillStyle = accentColor;
        ctx.fill();
      }
      ctx.globalAlpha = 1;

      /* ── Blips (staggered reveal) ── */
      const since = t - revealT;
      const labelGroups: Record<'left' | 'right', Array<{ b: _RBlip, bx: number, by: number, a: number, ty: number }>> = { left: [], right: [] };

      for (const b of blips) {
        if (since - b.revealAt < 0) continue;
        b.fadeIn = Math.min(b.fadeIn + 0.04, 1);
        b.locked = true;

        const bx = cx + Math.cos(b.angle) * (b.radius * maxR);
        const by = cy + Math.sin(b.angle) * (b.radius * maxR);
        const a = b.fadeIn;

        /* Glow */
        const gs = 50;
        ctx.globalAlpha = a * 0.5;
        ctx.drawImage(_glow(b.color), bx - gs / 2, by - gs / 2, gs, gs);
        ctx.globalAlpha = 1;

        /* Dot */
        ctx.beginPath();
        ctx.arc(bx, by, 3.5, 0, Math.PI * 2);
        ctx.fillStyle = b.color;
        ctx.globalAlpha = a;
        ctx.fill();

        /* Pulse ring */
        const pr = 6 + 3 * Math.sin(t * 3 + b.pulseOff);
        ctx.beginPath();
        ctx.arc(bx, by, pr, 0, Math.PI * 2);
        ctx.strokeStyle = b.color;
        ctx.lineWidth = 0.8;
        ctx.globalAlpha = a * 0.3;
        ctx.stroke();
        ctx.globalAlpha = 1;

        /* Collect labels for collision avoidance */
        if (a > 0.15) {
          const side = bx > cx ? 'right' : 'left';
          labelGroups[side].push({ b, bx, by, a, ty: by - 8 });
        }
      }

      const totalVisibleLabels = labelGroups.left.length + labelGroups.right.length;
      const labelFontSize =
        totalVisibleLabels <= 2 ? 13 :
        totalVisibleLabels <= 5 ? 12 :
        totalVisibleLabels <= 7 ? 11 :
        9;
      const labelIconRadius = labelFontSize <= 9 ? 2.5 : 2.5 + (labelFontSize - 9) * 0.4;
      const labelConnectorReach = labelFontSize <= 9 ? 14 : 14 + (labelFontSize - 9) * 2;
      const labelVerticalInset = labelFontSize <= 9 ? 8 : 10;

      /* ── Draw labels ── */
      for (const side of ['left', 'right'] as const) {
        const group = labelGroups[side];
        if (!group.length) continue;

        // Sort by natural y position
        group.sort((a, b) => a.ty - b.ty);

        // Distribute labels within cy ± maxR so they stay inside the radar
        // circle, not spread across the full canvas height.
        const distMin = Math.max(labelVerticalInset, cy - maxR);
        const distMax = Math.min(h - labelVerticalInset, cy + maxR);
        const n = group.length;
        for (let i = 0; i < n; i++) {
          group[i].ty = distMin + (i / Math.max(n - 1, 1)) * (distMax - distMin);
        }

        for (const { b, bx, by, a, ty } of group) {
          if (ty < 4 || ty > h - 4) continue;

          ctx.save();
          ctx.globalAlpha = a * 0.9;

          const enableVerb = /^(Enable|Show|Add)\s+/i.test(b.label);
          const cleanLabel = b.label
            .replace(/^(Disable|Enable|Block|Remove|Stop|Hide|Restrict|Nuke|Scrub|Show|Add)\s+/i, '')
            .toUpperCase();

          ctx.font = `500 ${labelFontSize}px monospace`;
          ctx.textBaseline = 'middle';
          const iconR = labelIconRadius;

          const drawIcon = (ix: number) => {
            ctx.strokeStyle = b.color;
            ctx.lineWidth = 1.1;
            if (enableVerb) {
              ctx.beginPath();
              ctx.moveTo(ix - iconR, ty); ctx.lineTo(ix + iconR, ty);
              ctx.moveTo(ix, ty - iconR); ctx.lineTo(ix, ty + iconR);
              ctx.stroke();
            } else {
              ctx.beginPath(); ctx.arc(ix, ty, iconR, 0, Math.PI * 2); ctx.stroke();
              ctx.beginPath(); ctx.moveTo(ix - 1.6, ty + 1.6); ctx.lineTo(ix + 1.6, ty - 1.6); ctx.stroke();
            }
          };

          if (side === 'right') {
            // Layout: blip → connector → [icon] [text →]
            const connTip = bx + labelConnectorReach;
            const ix = connTip + 4 + iconR;
            const textStartX = ix + iconR + 4;

            ctx.strokeStyle = b.color + '44';
            ctx.lineWidth = 0.6;
            ctx.beginPath();
            ctx.moveTo(bx + 5, by); ctx.lineTo(connTip, ty); ctx.stroke();

            drawIcon(ix);

            ctx.fillStyle = b.color;
            ctx.textAlign = 'left';
            ctx.fillText(cleanLabel, textStartX, ty);
          } else {
            // Layout: [icon] [text] ← connector ← blip
            // Icon is leftmost so it reads icon-then-text left to right.
            // Truncate labels that would push the icon off the left canvas edge.
            const connTip = bx - labelConnectorReach;
            const textEndX = connTip - 3; // text right edge, 3px gap from connector

            let display = cleanLabel;
            let tw = ctx.measureText(display).width;
            // Shrink until icon center (textEndX - tw - iconR - 4) fits within canvas
            while (textEndX - tw - iconR - 4 < 4 && display.length > 3) {
              display = display.slice(0, -1);
              tw = ctx.measureText(display + '…').width;
            }
            if (display !== cleanLabel) { display += '…'; tw = ctx.measureText(display).width; }
            const textStartX = textEndX - tw;
            const iconX = textStartX - iconR - 4; // icon center, left of text

            ctx.strokeStyle = b.color + '44';
            ctx.lineWidth = 0.6;
            ctx.beginPath();
            ctx.moveTo(bx - 5, by); ctx.lineTo(connTip, ty); ctx.stroke();

            if (iconX >= 3) drawIcon(iconX);

            ctx.fillStyle = b.color;
            ctx.textAlign = 'left';
            ctx.fillText(display, textStartX, ty);
          }

          ctx.restore();
        }
      }

      /* ── Center pulse ── */
      const cp = 0.7 + Math.sin(t * 3.5) * 0.3;
      const cgs = (isOptimal ? 48 : 36) * cp;
      ctx.globalAlpha = 0.55;
      ctx.drawImage(accentGlow, cx - cgs / 2, cy - cgs / 2, cgs, cgs);
      ctx.globalAlpha = 1;
      ctx.beginPath();
      ctx.arc(cx, cy, 4, 0, Math.PI * 2);
      ctx.fillStyle = accentColor;
      ctx.fill();

      /* ── Optimal Text ── */
      if (isOptimal) {
        ctx.save();
        ctx.font = '900 18px monospace';
        ctx.textAlign = 'center';
        ctx.textBaseline = 'middle';

        const textPulse = 0.8 + Math.sin(t * 2) * 0.2;
        ctx.shadowColor = '#4ed99c';
        ctx.shadowBlur = 12 * textPulse;

        ctx.fillStyle = isLight ? '#166534' : '#fff';
        ctx.fillText('SYSTEM OPTIMAL', cx, cy + 30);

        ctx.font = '700 10px monospace';
        ctx.fillStyle = isLight ? 'rgba(22,101,52,0.7)' : 'rgba(255,255,255,0.7)';
        ctx.shadowBlur = 0;
        ctx.fillText('ZERO THREATS DETECTED', cx, cy + 48);
        ctx.restore();
      }

      /* ── Pending app updates HUD ── */
      // Surface the count inside the radar so the action button itself can
      // stay clean ("Update All Apps") instead of carrying a counter.
      const pending = pendingAppUpdatesRef.current ?? 0;
      if (ph === 'complete' && pending > 0) {
        ctx.save();
        ctx.textAlign = 'center';
        ctx.textBaseline = 'middle';
        const amber = isLight ? '#92400e' : '#ff9500';
        const pulse = 0.7 + Math.sin(t * 2.4) * 0.3;
        ctx.shadowColor = amber;
        ctx.shadowBlur = 10 * pulse;
        ctx.font = '900 22px monospace';
        ctx.fillStyle = amber;
        ctx.fillText(String(pending), cx, cy + (isOptimal ? 78 : 30));
        ctx.shadowBlur = 0;
        ctx.font = '700 10px monospace';
        ctx.fillStyle = isLight ? 'rgba(146,64,14,0.78)' : 'rgba(255,149,0,0.82)';
        ctx.fillText(`APP UPDATE${pending === 1 ? '' : 'S'} PENDING`, cx, cy + (isOptimal ? 96 : 50));
        ctx.restore();
      }

      /* ── Corner HUD ── */
      ctx.font = '700 14px monospace';
      ctx.textAlign = 'left';
      ctx.textBaseline = 'top';
      if (ph === 'idle') {
        ctx.fillStyle = 'rgba(0,242,255,0.35)';
        ctx.fillText('READY', 14, 14);
      } else if (ph === 'scanning') {
        // scanning text removed per user request – keep corner blank during scan
      } else {
        // No text displayed when complete
      }

      raf = requestAnimationFrame(loop);
    };

    raf = requestAnimationFrame(loop);
    return () => { cancelAnimationFrame(raf); ro.disconnect(); };
  }, [height]);
  const style = transparent ? { ...containerStyle, background: 'transparent', pointerEvents: 'none' as const } : containerStyle;

  return (
    <div className="radar-container" style={style}>
      <canvas ref={ref} />
    </div>
  );
}
