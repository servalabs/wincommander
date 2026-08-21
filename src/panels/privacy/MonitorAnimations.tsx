// src/panels/privacy/MonitorAnimations.tsx
//
// Canvas-based "How it works" animations for the four monitor-style
// features (Paste / Decoy / Ransomware / Panic-Keyword). Each component:
//   1. Auto-switches BG + text palette via useTheme() so the canvas
//      matches whichever theme the user is on.
//   2. Loops on a fixed cycle for predictability.
//   3. Exposes a `triggerFromInput` hook so the parent dialog can hand
//      in a real string (paste / typed) and short-circuit straight to
//      the "alarm" phase — that's the interactive bit.
//
// Shared infrastructure lives at the top.

import { useEffect, useImperativeHandle, useRef, forwardRef } from 'react';
import { useTheme } from '../../context/ThemeContext';
import useMotionPreference from '@/hooks/useMotionPreference';

// ── Shared utilities ───────────────────────────────────────────────
const ANIM_W = 600;
const ANIM_H = 340;
const FRAME_MS = 1000 / 30;

interface Palette {
  bg: string;
  card: string;
  cardBorder: string;
  textPrimary: string;
  textMuted: string;
  accent: string;
  accentSoft: string;
  danger: string;
  warn: string;
  ok: string;
  fileFill: string;
  fileFillLocked: string;
  paper: string;
}

function paletteFor(theme: 'dark' | 'light'): Palette {
  if (theme === 'light') {
    return {
      bg: '#eef3f9',
      card: '#ffffff',
      cardBorder: 'rgba(15,23,42,0.12)',
      textPrimary: '#111827',
      textMuted: '#475569',
      accent: '#0ea5e9',
      accentSoft: 'rgba(14,165,233,0.15)',
      danger: '#dc2626',
      warn: '#d97706',
      ok: '#059669',
      fileFill: '#e5edf5',
      fileFillLocked: '#fee2e2',
      paper: '#ffffff',
    };
  }
  return {
    bg: '#0d1422',
    card: '#1a2540',
    cardBorder: 'rgba(255,255,255,0.15)',
    textPrimary: '#e2e8f0',
    textMuted: '#94a3b8',
    accent: '#00f2ff',
    accentSoft: 'rgba(0,242,255,0.15)',
    danger: '#ff3030',
    warn: '#ffaa00',
    ok: '#3ad29f',
    fileFill: '#2a3a55',
    fileFillLocked: '#3a1a1a',
    paper: '#ffffff',
  };
}

function initCanvas(c: HTMLCanvasElement) {
  try {
    const d = Math.min(window.devicePixelRatio ?? 1, 2);
    c.width = ANIM_W * d;
    c.height = ANIM_H * d;
    c.style.cssText = 'display:block;max-width:100%;max-height:100%;object-fit:contain;';
    const ctx = c.getContext('2d');
    if (!ctx) return { ctx: null, w: 0, h: 0 };
    ctx.scale(d, d);
    return { ctx, w: ANIM_W, h: ANIM_H };
  } catch {
    return { ctx: null, w: 0, h: 0 };
  }
}

function makeGlow(r: number, g: number, b: number, sz = 240) {
  const c = document.createElement('canvas');
  c.width = c.height = sz;
  const x = c.getContext('2d');
  if (!x) return c;
  const half = sz / 2;
  const gr = x.createRadialGradient(half, half, 0, half, half, half);
  gr.addColorStop(0, `rgba(${r},${g},${b},0.55)`);
  gr.addColorStop(0.4, `rgba(${r},${g},${b},0.1)`);
  gr.addColorStop(1, 'transparent');
  x.fillStyle = gr;
  x.fillRect(0, 0, sz, sz);
  return c;
}

function easeOut(v: number) { return 1 - Math.pow(1 - v, 3); }

// Drawing helpers
function roundRect(ctx: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, r: number) {
  ctx.beginPath();
  if (ctx.roundRect) ctx.roundRect(x, y, w, h, r);
  else { ctx.rect(x, y, w, h); }
}

function drawFile(ctx: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, color: string, locked: boolean, p: Palette) {
  ctx.fillStyle = color;
  ctx.strokeStyle = locked ? p.danger : p.cardBorder;
  ctx.lineWidth = 1.5;
  roundRect(ctx, x, y, w, h, 3);
  ctx.fill();
  ctx.stroke();
  // Folded corner
  ctx.fillStyle = locked ? 'rgba(220,38,38,0.25)' : 'rgba(0,0,0,0.06)';
  ctx.beginPath();
  ctx.moveTo(x + w - 8, y);
  ctx.lineTo(x + w, y);
  ctx.lineTo(x + w, y + 8);
  ctx.closePath();
  ctx.fill();
  if (locked) {
    ctx.fillStyle = p.danger;
    const cx = x + w / 2;
    const cy = y + h / 2;
    ctx.beginPath();
    ctx.arc(cx, cy - 3, 3.5, Math.PI, 2 * Math.PI);
    ctx.lineWidth = 2;
    ctx.strokeStyle = p.danger;
    ctx.stroke();
    ctx.fillRect(cx - 4.5, cy - 2, 9, 8);
  }
}

// RAF driver with visibility gating.
// Returns a { stop, startIfActive } controller so callers can wire up
// IntersectionObserver and visibilitychange without duplicating RAF logic.
function makeLoop(draw: (t: number) => void) {
  let raf = 0;
  let lastTs = 0;
  let isIntersecting = true;
  let isDocVisible = !document.hidden;
  const t0 = performance.now() / 1000;

  const loop = (ts: number) => {
    if (ts - lastTs < FRAME_MS) { raf = requestAnimationFrame(loop); return; }
    lastTs = ts;
    draw(performance.now() / 1000 - t0);
    raf = requestAnimationFrame(loop);
  };

  const stop = () => { if (raf !== 0) { cancelAnimationFrame(raf); raf = 0; } };

  const startIfActive = () => {
    // Only schedule when the canvas is on-screen and the tab is visible.
    if (isIntersecting && isDocVisible && raf === 0) {
      raf = requestAnimationFrame(loop);
    }
  };

  const setIntersecting = (v: boolean) => { isIntersecting = v; };
  const setDocVisible   = (v: boolean) => { isDocVisible   = v; };

  return { stop, startIfActive, setIntersecting, setDocVisible };
}

// Shared interactive-trigger ref API.
export interface MonitorAnimationHandle {
  /** Forcibly run the alarm phase for the given seconds, ignoring the
   *  cycle. Used when a parent text-input "simulates" a real trigger. */
  fireAlarm(holdSeconds?: number): void;
}

// ───────────────────────────────────────────────────────────────────
// 1. Paste Monitor — clipboard pattern scanner
// ───────────────────────────────────────────────────────────────────
export const PasteMonitorAnimation = forwardRef<MonitorAnimationHandle, {}>((_, ref) => {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const overrideRef = useRef<number | null>(null);
  const { theme } = useTheme();
  const motion = useMotionPreference();

  useImperativeHandle(ref, () => ({
    fireAlarm(hold = 3) {
      overrideRef.current = performance.now() / 1000 + hold;
    },
  }), []);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const { ctx, w, h } = initCanvas(canvas);
    if (!ctx) return;
    const p = paletteFor(theme);
    const redGlow = makeGlow(255, 30, 30, 280);
    const accentGlow = makeGlow(0, 200, 255, 220);
    const CYCLE = 9;

    // draw() is extracted so the reduced-motion path can paint one idle frame.
    const draw = (t: number) => {
      const tc = t % CYCLE;
      let phase: 0 | 1 | 2 = 0;
      let pp = 0;
      // Override: external trigger forces phase 2 for hold time.
      const now = performance.now() / 1000;
      if (overrideRef.current !== null && now < overrideRef.current) {
        phase = 2;
        pp = 1;
      } else {
        if (overrideRef.current !== null && now >= overrideRef.current) overrideRef.current = null;
        if (tc < 2) { phase = 0; pp = tc / 2; }
        else if (tc < 5) { phase = 1; pp = (tc - 2) / 3; }
        else if (tc < 7) { phase = 2; pp = (tc - 5) / 2; }
        else { phase = 0; pp = 1 - (tc - 7) / 2; }
      }
      // pp drives content reveal + scan line; no separate easeOut needed.

      ctx.fillStyle = p.bg; ctx.fillRect(0, 0, w, h);

      // Center group: clipboard + pattern engine side-by-side
      const cx = w / 2;
      const totalW = 440;
      const groupX = cx - totalW / 2;
      const clipW = 260, clipH = 200;
      const clipX = groupX;
      const clipY = h / 2 - clipH / 2 + 14;

      // Alarm glow behind everything
      if (phase === 2) {
        ctx.globalAlpha = 0.7 + 0.3 * Math.sin(tc * 8);
        ctx.drawImage(redGlow, cx - 140, h / 2 - 130);
        ctx.globalAlpha = 1;
      }

      // ── Clipboard ──
      ctx.fillStyle = p.card;
      ctx.strokeStyle = phase === 2 ? p.danger : p.accent;
      ctx.lineWidth = 2;
      roundRect(ctx, clipX, clipY, clipW, clipH, 10);
      ctx.fill();
      ctx.stroke();
      // Clip top
      ctx.fillStyle = phase === 2 ? p.danger : p.accent;
      roundRect(ctx, clipX + clipW / 2 - 32, clipY - 9, 64, 18, 5);
      ctx.fill();

      // Paper
      ctx.fillStyle = p.paper;
      ctx.fillRect(clipX + 14, clipY + 18, clipW - 28, clipH - 32);
      ctx.strokeStyle = 'rgba(0,0,0,0.05)';
      ctx.strokeRect(clipX + 14, clipY + 18, clipW - 28, clipH - 32);

      // Content
      ctx.fillStyle = '#1f2937';
      ctx.font = '11px ui-monospace, monospace';
      if (phase === 0) {
        ctx.fillStyle = '#94a3b8';
        ctx.fillText('clipboard ready', clipX + 24, clipY + 50);
        ctx.font = '10px ui-monospace, monospace';
        ctx.fillText('waiting for copy event…', clipX + 24, clipY + 70);
      } else if (phase === 1) {
        const sample = 'AKIA5HFGPN7VQXTK9ZM2';
        const reveal = Math.floor(pp * sample.length);
        ctx.fillStyle = '#1f2937';
        ctx.font = 'bold 12px ui-monospace, monospace';
        ctx.fillText(sample.slice(0, reveal), clipX + 24, clipY + 50);
        ctx.font = '10px ui-monospace, monospace';
        ctx.fillStyle = '#94a3b8';
        ctx.fillText('analyzing pattern…', clipX + 24, clipY + 72);
        // Scan line
        const scanY = clipY + 22 + ((tc * 180) % (clipH - 40));
        ctx.fillStyle = `rgba(14, 165, 233, ${0.4 + 0.4 * Math.sin(tc * 6)})`;
        ctx.fillRect(clipX + 14, scanY, clipW - 28, 2);
      } else {
        ctx.fillStyle = p.danger;
        ctx.font = 'bold 12px ui-monospace, monospace';
        ctx.fillText('CREDENTIAL DETECTED', clipX + 24, clipY + 50);
        ctx.font = '10px ui-monospace, monospace';
        ctx.fillStyle = '#94a3b8';
        ctx.fillText('clipboard cleared + lockdown ready', clipX + 24, clipY + 72);
        // Strikethrough effect on what was copied
        ctx.strokeStyle = p.danger;
        ctx.lineWidth = 1.5;
        ctx.beginPath();
        ctx.moveTo(clipX + 22, clipY + 95);
        ctx.lineTo(clipX + clipW - 22, clipY + 95);
        ctx.stroke();
      }

      // ── Pattern engine ──
      const peX = clipX + clipW + 20;
      const peY = clipY + 12;
      const peW = 160, peH = 176;
      ctx.fillStyle = p.card;
      ctx.strokeStyle = phase === 2 ? p.danger : (phase === 1 ? p.accent : p.cardBorder);
      ctx.lineWidth = 1.5;
      roundRect(ctx, peX, peY, peW, peH, 8);
      ctx.fill();
      ctx.stroke();

      if (phase === 1 && pp > 0.3) {
        ctx.drawImage(accentGlow, peX - 40, peY - 30);
      }

      ctx.fillStyle = p.textPrimary;
      ctx.font = 'bold 11px ui-monospace, monospace';
      ctx.fillText('PATTERN ENGINE', peX + 12, peY + 22);

      const patterns = ['AWS_KEY', 'JWT', 'BIP-39', 'Private Key', 'Cred File'];
      patterns.forEach((pat, i) => {
        const py = peY + 46 + i * 24;
        const active = phase === 1 && i === 0;
        const matched = phase === 2 && i === 0;
        ctx.fillStyle = matched ? p.danger : active ? p.accent : p.textMuted;
        ctx.font = `${active || matched ? 'bold ' : ''}10px ui-monospace, monospace`;
        ctx.fillText(matched ? '✗' : active ? '▶' : '▸', peX + 12, py);
        ctx.fillText(pat, peX + 28, py);
        if (matched) {
          ctx.fillStyle = p.danger;
          ctx.font = '9px ui-monospace, monospace';
          ctx.fillText('MATCH', peX + peW - 38, py);
        }
      });

      // Alarm banner
      if (phase === 2) {
        ctx.fillStyle = `rgba(220, 38, 38, ${0.85 + 0.15 * Math.sin(tc * 10)})`;
        ctx.fillRect(0, 22, w, 36);
        ctx.fillStyle = '#fff';
        ctx.font = 'bold 13px ui-monospace, monospace';
        const txt = '⚠  SECRET DETECTED — CLIPBOARD CLEARED  ⚠';
        const m = ctx.measureText(txt).width;
        ctx.fillText(txt, w / 2 - m / 2, 45);
      } else {
        const status = phase === 1 ? 'SCANNING…' : 'WATCHING CLIPBOARD';
        ctx.fillStyle = phase === 1 ? p.accent : p.textMuted;
        ctx.font = 'bold 11px ui-monospace, monospace';
        const m = ctx.measureText(status).width;
        ctx.fillText(status, w / 2 - m / 2, 32);
      }
    };

    // Reduced-motion: one static idle frame, no loop.
    if (motion === 'reduced') { draw(0); return; }

    const ctrl = makeLoop(draw);

    // Pause when the canvas leaves the viewport; resume on re-entry.
    const io = new IntersectionObserver(
      ([entry]) => {
        ctrl.setIntersecting(entry.isIntersecting);
        if (entry.isIntersecting) ctrl.startIfActive();
        else ctrl.stop();
      },
      { threshold: 0 }
    );
    io.observe(canvas);

    // Pause when the tab/window goes to background; resume when visible.
    const onVisibility = () => {
      ctrl.setDocVisible(!document.hidden);
      if (!document.hidden) ctrl.startIfActive();
      else ctrl.stop();
    };
    document.addEventListener('visibilitychange', onVisibility);

    ctrl.startIfActive();

    return () => {
      ctrl.stop();
      io.disconnect();
      document.removeEventListener('visibilitychange', onVisibility);
    };
  }, [theme, motion]);
  return <canvas ref={canvasRef} aria-label="Paste monitor animation" />;
});
PasteMonitorAnimation.displayName = 'PasteMonitorAnimation';

// ───────────────────────────────────────────────────────────────────
// 2. File Access Monitor — TWO LAPTOPS: user PC (decoy accessed) → admin PC
// ───────────────────────────────────────────────────────────────────
//
// Visual concept (preferred over the earlier folder-only view):
//   - Left laptop = the user's PC where the decoy file lives.
//   - Right laptop = the admin's PC that monitors the fleet.
//   - A network connection line runs between them.
//   - When the decoy is touched on the user PC, a glowing data
//     packet flies along the connection line to the admin PC, and
//     the admin PC's screen flips to a red ALERT view.
//
// Phases (8 s cycle):
//   0–1.5  idle — both PCs normal, decoy visible on user screen
//   1.5–4  intruder cursor on user PC moves toward the decoy file
//   4–5.5  decoy is touched → packet travels left→right along link
//   5.5–7  admin PC screen flashes ALERT, sustains
//   7–8    fade back to idle
function drawLaptop(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  w: number,
  h: number,
  alert: boolean,
  p: Palette
) {
  // Screen frame
  ctx.fillStyle = '#1a2540';
  ctx.strokeStyle = alert ? p.danger : 'rgba(148,163,184,0.5)';
  ctx.lineWidth = 2;
  roundRect(ctx, x, y, w, h, 8);
  ctx.fill();
  ctx.stroke();
  // Webcam dot at top-center of bezel
  ctx.beginPath();
  ctx.arc(x + w / 2, y + 5, 1.6, 0, Math.PI * 2);
  ctx.fillStyle = alert ? p.danger : 'rgba(0,200,255,0.6)';
  ctx.fill();
  // Base trapezoid
  const baseY = y + h;
  ctx.fillStyle = '#162033';
  ctx.strokeStyle = 'rgba(148,163,184,0.4)';
  ctx.beginPath();
  ctx.moveTo(x - 14, baseY);
  ctx.lineTo(x + w + 14, baseY);
  ctx.lineTo(x + w + 22, baseY + 12);
  ctx.lineTo(x - 22, baseY + 12);
  ctx.closePath();
  ctx.fill();
  ctx.stroke();
  // Trackpad indent
  ctx.fillStyle = 'rgba(148,163,184,0.2)';
  ctx.fillRect(x + w / 2 - 18, baseY + 3, 36, 5);
}

export const DecoyMonitorAnimation = forwardRef<MonitorAnimationHandle, {}>((_, ref) => {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const overrideRef = useRef<number | null>(null);
  const { theme } = useTheme();
  const motion = useMotionPreference();

  useImperativeHandle(ref, () => ({
    fireAlarm(hold = 3) {
      overrideRef.current = performance.now() / 1000 + hold;
    },
  }), []);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const { ctx, w, h } = initCanvas(canvas);
    if (!ctx) return;
    const p = paletteFor(theme);
    const redGlow = makeGlow(255, 30, 30, 220);
    const amberGlow = makeGlow(255, 170, 0, 160);
    const accentGlow = makeGlow(0, 200, 255, 160);
    const CYCLE = 8;

    const draw = (t: number) => {
      const tc = t % CYCLE;
      // Phases: 0 idle, 1 cursor moving, 2 packet flying, 3 alert held
      let phase: 0 | 1 | 2 | 3 = 0;
      let pp = 0;
      const now = performance.now() / 1000;
      if (overrideRef.current !== null && now < overrideRef.current) {
        phase = 3; pp = 1;
      } else {
        if (overrideRef.current !== null && now >= overrideRef.current) overrideRef.current = null;
        if (tc < 1.5) { phase = 0; pp = tc / 1.5; }
        else if (tc < 4) { phase = 1; pp = (tc - 1.5) / 2.5; }
        else if (tc < 5.5) { phase = 2; pp = (tc - 4) / 1.5; }
        else if (tc < 7) { phase = 3; pp = (tc - 5.5) / 1.5; }
        else { phase = 0; pp = 0; }
      }
      const pe = easeOut(pp);

      ctx.fillStyle = p.bg; ctx.fillRect(0, 0, w, h);

      // ── Two laptops ──
      const lapW = 240, lapH = 156;
      const leftX = 30, rightX = w - 30 - lapW;
      const lapY = h / 2 - lapH / 2 + 12;
      const userAlert = phase === 1 || phase === 2 || phase === 3;
      const adminAlert = phase === 3;
      drawLaptop(ctx, leftX, lapY, lapW, lapH, userAlert, p);
      drawLaptop(ctx, rightX, lapY, lapW, lapH, adminAlert, p);

      // Labels (above each laptop)
      ctx.fillStyle = p.textMuted;
      ctx.font = 'bold 10px ui-monospace, monospace';
      ctx.fillText('USER PC', leftX + 6, lapY - 8);
      ctx.fillText('ADMIN PC', rightX + 6, lapY - 8);

      // ── User PC screen content ──
      // Inner screen region
      const scrPad = 10;
      const usX = leftX + scrPad, usY = lapY + scrPad;
      const usW = lapW - 2 * scrPad, usH = lapH - 2 * scrPad;
      ctx.save();
      ctx.beginPath();
      ctx.rect(usX, usY, usW, usH);
      ctx.clip();
      ctx.fillStyle = userAlert ? '#3d1212' : '#0f1a2e';
      ctx.fillRect(usX, usY, usW, usH);

      // Faux file manager rows + the decoy file row
      const rowH = 18;
      const files = ['notes.txt', 'budget.xlsx', 'SECRET.docx', 'meeting.md', 'photos.zip', 'todo.txt'];
      const decoyIdx = 2;
      for (let i = 0; i < files.length; i++) {
        const y = usY + 14 + i * rowH;
        if (y > usY + usH - 6) break;
        const isDecoy = i === decoyIdx;
        // Decoy row highlight pulse
        if (isDecoy) {
          const pulse = phase === 0 ? 0.25 + 0.2 * Math.sin(t * 2.5)
            : phase >= 2 ? 0.55 : 0.4;
          ctx.fillStyle = userAlert ? `rgba(220,38,38,${pulse})` : `rgba(255,170,0,${pulse})`;
          ctx.fillRect(usX + 2, y - 13, usW - 4, rowH - 2);
        }
        ctx.fillStyle = isDecoy ? '#fff' : '#bcd0ea';
        ctx.font = `${isDecoy ? 'bold ' : ''}11px ui-monospace, monospace`;
        ctx.fillText(`📄 ${files[i]}`, usX + 8, y);
      }

      // Decoy row position — used by cursor + packet origin
      const decoyRowY = usY + 14 + decoyIdx * rowH - 6;
      const decoyClickX = usX + 80;        // where cursor lands
      const decoyClickY = decoyRowY + 6;

      // Intruder cursor on user PC — start outside the screen at top-right,
      // move to the SECRET.docx row.
      if (phase === 1 || phase === 2 || phase === 3) {
        const startX = usX + usW - 6;
        const startY = usY + 6;
        const t01 = phase === 1 ? pe : 1; // arrive by end of phase 1
        const handX = startX + (decoyClickX - startX) * t01;
        const handY = startY + (decoyClickY - startY) * t01;
        ctx.save();
        ctx.translate(handX, handY);
        ctx.rotate(-Math.PI / 7);
        ctx.fillStyle = userAlert ? p.danger : p.warn;
        ctx.strokeStyle = '#000';
        ctx.lineWidth = 1;
        ctx.beginPath();
        ctx.moveTo(0, 0);
        ctx.lineTo(0, 18);
        ctx.lineTo(4, 14);
        ctx.lineTo(8, 22);
        ctx.lineTo(11, 20);
        ctx.lineTo(7, 13);
        ctx.lineTo(12, 11);
        ctx.closePath();
        ctx.fill();
        ctx.stroke();
        ctx.restore();
      }

      // Subtle red glow when alert on user PC
      if (userAlert) {
        ctx.globalAlpha = phase === 1 ? 0.35 : 0.55;
        ctx.drawImage(redGlow, decoyClickX - 110, decoyClickY - 110);
        ctx.globalAlpha = 1;
      } else {
        ctx.globalAlpha = 0.4;
        ctx.drawImage(amberGlow, decoyClickX - 80, decoyClickY - 80);
        ctx.globalAlpha = 1;
      }

      ctx.restore(); // end clip for user screen

      // ── Network link between laptops ──
      // Solid faint base line + small endpoint dots
      const linkY = lapY + lapH / 2;
      const linkX1 = leftX + lapW + 2;
      const linkX2 = rightX - 2;
      ctx.strokeStyle = phase >= 2 ? 'rgba(220,38,38,0.5)' : 'rgba(148,163,184,0.45)';
      ctx.setLineDash([4, 4]);
      ctx.lineWidth = 1.5;
      ctx.beginPath();
      ctx.moveTo(linkX1, linkY);
      ctx.lineTo(linkX2, linkY);
      ctx.stroke();
      ctx.setLineDash([]);
      // Endpoint dots
      ctx.fillStyle = phase >= 2 ? p.danger : p.textMuted;
      ctx.beginPath();
      ctx.arc(linkX1, linkY, 3, 0, Math.PI * 2);
      ctx.fill();
      ctx.beginPath();
      ctx.arc(linkX2, linkY, 3, 0, Math.PI * 2);
      ctx.fill();

      // ── Data packet flying along the link in phase 2 ──
      if (phase === 2) {
        const packX = linkX1 + (linkX2 - linkX1) * pe;
        // Glow halo
        ctx.globalAlpha = 0.9;
        ctx.drawImage(redGlow, packX - 110, linkY - 110, 220, 220);
        ctx.globalAlpha = 1;
        // Packet body — small red square with a tail
        ctx.fillStyle = p.danger;
        ctx.fillRect(packX - 4, linkY - 4, 8, 8);
        ctx.strokeStyle = p.danger;
        ctx.lineWidth = 1.5;
        ctx.beginPath();
        ctx.moveTo(packX - 12, linkY);
        ctx.lineTo(packX - 4, linkY);
        ctx.stroke();
        // "ALERT" label hovering above the packet
        ctx.fillStyle = p.danger;
        ctx.font = 'bold 9px ui-monospace, monospace';
        ctx.fillText('ALERT', packX - 14, linkY - 10);
      }
      // After packet arrival (phase 3), keep a faint trail
      if (phase === 3) {
        ctx.strokeStyle = `rgba(220,38,38,${0.5 + 0.3 * Math.sin(tc * 8)})`;
        ctx.lineWidth = 2;
        ctx.setLineDash([6, 4]);
        ctx.beginPath();
        ctx.moveTo(linkX1, linkY);
        ctx.lineTo(linkX2, linkY);
        ctx.stroke();
        ctx.setLineDash([]);
      }

      // ── Admin PC screen content ──
      const asX = rightX + scrPad, asY = lapY + scrPad;
      const asW = lapW - 2 * scrPad, asH = lapH - 2 * scrPad;
      ctx.save();
      ctx.beginPath();
      ctx.rect(asX, asY, asW, asH);
      ctx.clip();
      ctx.fillStyle = adminAlert ? '#3d1212' : '#0f1a2e';
      ctx.fillRect(asX, asY, asW, asH);

      if (!adminAlert) {
        // Idle admin dashboard — header + a couple of status rows
        ctx.fillStyle = '#9bcfff';
        ctx.font = 'bold 11px ui-monospace, monospace';
        ctx.fillText('FLEET MONITOR', asX + 8, asY + 18);
        ctx.fillStyle = p.ok;
        ctx.font = '10px ui-monospace, monospace';
        ctx.fillText('● 47 devices ok', asX + 8, asY + 36);
        ctx.fillStyle = p.textMuted;
        ctx.fillText('— no incidents —', asX + 8, asY + 52);
        // Tiny pulse dot to show "live monitoring"
        const dotA = 0.4 + 0.5 * Math.sin(t * 3);
        ctx.fillStyle = `rgba(58, 210, 159, ${dotA})`;
        ctx.beginPath();
        ctx.arc(asX + asW - 14, asY + 16, 4, 0, Math.PI * 2);
        ctx.fill();
        // Admin screen accent glow
        ctx.globalAlpha = 0.25;
        ctx.drawImage(accentGlow, asX + asW / 2 - 80, asY + asH / 2 - 80);
        ctx.globalAlpha = 1;
      } else {
        // Alert view on admin PC
        const flash = 0.85 + 0.15 * Math.sin(tc * 10);
        ctx.fillStyle = `rgba(220, 38, 38, ${flash})`;
        ctx.fillRect(asX, asY, asW, 28);
        ctx.fillStyle = '#fff';
        ctx.font = 'bold 11px ui-monospace, monospace';
        ctx.fillText('⚠ INCIDENT', asX + 8, asY + 19);
        // Incident card
        ctx.fillStyle = 'rgba(220, 38, 38, 0.18)';
        ctx.fillRect(asX + 6, asY + 36, asW - 12, asH - 44);
        ctx.strokeStyle = p.danger;
        ctx.lineWidth = 1;
        ctx.strokeRect(asX + 6, asY + 36, asW - 12, asH - 44);
        ctx.fillStyle = '#fff';
        ctx.font = '10px ui-monospace, monospace';
        ctx.fillText('User PC · just now', asX + 12, asY + 52);
        ctx.fillStyle = '#ffd1d1';
        ctx.fillText('Decoy file SECRET.docx', asX + 12, asY + 68);
        ctx.fillText('was opened.', asX + 12, asY + 82);
        ctx.fillStyle = '#fff';
        ctx.font = 'bold 9px ui-monospace, monospace';
        ctx.fillText('▸ ESCALATE', asX + 12, asY + asH - 14);
        ctx.fillStyle = '#ffaaaa';
        ctx.fillText('▸ ACK', asX + 90, asY + asH - 14);
      }
      ctx.restore(); // end clip for admin screen

      // ── Banner across the top ──
      if (phase === 3) {
        ctx.fillStyle = `rgba(220, 38, 38, ${0.85 + 0.15 * Math.sin(tc * 10)})`;
        ctx.fillRect(0, 12, w, 32);
        ctx.fillStyle = '#fff';
        ctx.font = 'bold 13px ui-monospace, monospace';
        const txt = '⚠  DECOY ACCESSED — ADMIN NOTIFIED  ⚠';
        const m = ctx.measureText(txt).width;
        ctx.fillText(txt, w / 2 - m / 2, 32);
      } else if (phase === 2) {
        ctx.fillStyle = p.danger;
        ctx.font = 'bold 11px ui-monospace, monospace';
        const txt = 'TRANSMITTING ALERT TO ADMIN…';
        const m = ctx.measureText(txt).width;
        ctx.fillText(txt, w / 2 - m / 2, 28);
      } else {
        ctx.fillStyle = phase === 1 ? p.warn : p.textMuted;
        ctx.font = 'bold 11px ui-monospace, monospace';
        const status = phase === 1 ? 'INTRUDER OPENING DECOY' : 'FLEET MONITORING - FILE WATCH ACTIVE';
        const m = ctx.measureText(status).width;
        ctx.fillText(status, w / 2 - m / 2, 28);
      }
    };

    // Reduced-motion: one static idle frame, no loop.
    if (motion === 'reduced') { draw(0); return; }

    const ctrl = makeLoop(draw);

    const io = new IntersectionObserver(
      ([entry]) => {
        ctrl.setIntersecting(entry.isIntersecting);
        if (entry.isIntersecting) ctrl.startIfActive();
        else ctrl.stop();
      },
      { threshold: 0 }
    );
    io.observe(canvas);

    const onVisibility = () => {
      ctrl.setDocVisible(!document.hidden);
      if (!document.hidden) ctrl.startIfActive();
      else ctrl.stop();
    };
    document.addEventListener('visibilitychange', onVisibility);

    ctrl.startIfActive();

    return () => {
      ctrl.stop();
      io.disconnect();
      document.removeEventListener('visibilitychange', onVisibility);
    };
  }, [theme, motion]);
  return <canvas ref={canvasRef} aria-label="Decoy monitor animation" />;
});
DecoyMonitorAnimation.displayName = 'DecoyMonitorAnimation';

// ───────────────────────────────────────────────────────────────────
// 3. Ransomware Monitor — improved: lock-spread, dramatic meter
// ───────────────────────────────────────────────────────────────────
export const RansomwareMonitorAnimation = forwardRef<MonitorAnimationHandle, {}>((_, ref) => {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const overrideRef = useRef<number | null>(null);
  const { theme } = useTheme();
  const motion = useMotionPreference();

  useImperativeHandle(ref, () => ({
    fireAlarm(hold = 3) {
      overrideRef.current = performance.now() / 1000 + hold;
    },
  }), []);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const { ctx, w, h } = initCanvas(canvas);
    if (!ctx) return;
    const p = paletteFor(theme);
    const redGlow = makeGlow(255, 30, 30, 320);
    const FILE_COUNT = 12;
    const THRESHOLD = 7;
    const CYCLE = 10;

    const draw = (t: number) => {
      const tc = t % CYCLE;
      let phase: 0 | 1 | 2 = 0;
      let pp = 0;
      const now = performance.now() / 1000;
      if (overrideRef.current !== null && now < overrideRef.current) {
        phase = 2; pp = 1;
      } else {
        if (overrideRef.current !== null && now >= overrideRef.current) overrideRef.current = null;
        if (tc < 1.5) { phase = 0; pp = tc / 1.5; }
        else if (tc < 6.5) { phase = 1; pp = (tc - 1.5) / 5; }
        else if (tc < 8.5) { phase = 2; pp = (tc - 6.5) / 2; }
        else { phase = 0; pp = 0; }
      }

      // How many files appear locked
      let encrypted = 0;
      if (phase === 1) encrypted = Math.min(THRESHOLD, Math.floor(pp * (THRESHOLD + 1)));
      else if (phase === 2) encrypted = FILE_COUNT;
      const meterFill = Math.min(encrypted / THRESHOLD, 1);

      ctx.fillStyle = p.bg; ctx.fillRect(0, 0, w, h);

      if (phase === 2) {
        ctx.globalAlpha = 0.6 + 0.4 * Math.sin(tc * 10);
        ctx.drawImage(redGlow, w / 2 - 160, h / 2 - 130);
        ctx.globalAlpha = 1;
      }

      // ── Threshold meter (top) ──
      const meterX = 80, meterY = 60;
      const meterW = w - 160, meterH = 22;
      ctx.fillStyle = p.card;
      ctx.strokeStyle = p.cardBorder;
      ctx.lineWidth = 1.5;
      roundRect(ctx, meterX, meterY, meterW, meterH, 11);
      ctx.fill();
      ctx.stroke();

      // Animated gradient fill
      const fillW = meterW * meterFill;
      const fillColor = meterFill < 0.5 ? p.ok : meterFill < 0.95 ? p.warn : p.danger;
      ctx.fillStyle = fillColor;
      roundRect(ctx, meterX, meterY, Math.max(fillW, 6), meterH, 11);
      ctx.fill();

      // Threshold tick + label
      const tickX = meterX + (meterW * THRESHOLD) / FILE_COUNT;
      ctx.strokeStyle = p.textPrimary;
      ctx.setLineDash([3, 3]);
      ctx.lineWidth = 1.5;
      ctx.beginPath();
      ctx.moveTo(tickX, meterY - 8);
      ctx.lineTo(tickX, meterY + meterH + 8);
      ctx.stroke();
      ctx.setLineDash([]);

      ctx.fillStyle = p.textPrimary;
      ctx.font = 'bold 11px ui-monospace, monospace';
      ctx.fillText(`MASS-ENCRYPTION RATE`, meterX, meterY - 12);
      ctx.fillStyle = p.textMuted;
      ctx.font = '10px ui-monospace, monospace';
      ctx.textAlign = 'right';
      ctx.fillText(`${encrypted} / ${FILE_COUNT} files in 5 s`, meterX + meterW, meterY - 12);
      ctx.textAlign = 'left';
      ctx.fillStyle = p.textPrimary;
      ctx.font = 'bold 9px ui-monospace, monospace';
      ctx.fillText(`THRESHOLD`, tickX - 28, meterY + meterH + 22);

      // ── File grid: 2 rows × 6 cols ──
      const cols = 6;
      const fileW = 48, fileH = 60;
      const gapX = 14, gapY = 14;
      const gridW = cols * fileW + (cols - 1) * gapX;
      const gridX = w / 2 - gridW / 2;
      const gridY = meterY + meterH + 50;
      for (let i = 0; i < FILE_COUNT; i++) {
        const c = i % cols;
        const r = Math.floor(i / cols);
        const fx = gridX + c * (fileW + gapX);
        const fy = gridY + r * (fileH + gapY);
        const locked = i < encrypted;
        // Newly locked file gets a brief scale-up + flash
        if (phase === 1 && i === encrypted - 1) {
          const sub = (pp * (THRESHOLD + 1)) - encrypted + 1; // 0..1 since last increment
          const scale = 1 + 0.15 * (1 - Math.abs(sub - 0.5) * 2);
          ctx.save();
          ctx.translate(fx + fileW / 2, fy + fileH / 2);
          ctx.scale(scale, scale);
          ctx.translate(-(fx + fileW / 2), -(fy + fileH / 2));
          drawFile(ctx, fx, fy, fileW, fileH, p.fileFillLocked, true, p);
          ctx.restore();
        } else {
          drawFile(ctx, fx, fy, fileW, fileH, locked ? p.fileFillLocked : p.fileFill, locked, p);
        }
      }

      // Banner
      if (phase === 2) {
        ctx.fillStyle = `rgba(220, 38, 38, ${0.85 + 0.15 * Math.sin(tc * 10)})`;
        ctx.fillRect(0, h - 44, w, 38);
        ctx.fillStyle = '#fff';
        ctx.font = 'bold 13px ui-monospace, monospace';
        const txt = '⚠  RANSOMWARE PATTERN — TRIGGER FIRED  ⚠';
        const m = ctx.measureText(txt).width;
        ctx.fillText(txt, w / 2 - m / 2, h - 20);
      } else {
        ctx.fillStyle = phase === 1 ? p.warn : p.ok;
        ctx.font = 'bold 11px ui-monospace, monospace';
        const status = phase === 1 ? `ENCRYPTION DETECTED — ${encrypted}/${THRESHOLD}` : 'MONITORING FILE SYSTEM';
        const m = ctx.measureText(status).width;
        ctx.fillText(status, w / 2 - m / 2, h - 18);
      }
    };

    // Reduced-motion: one static idle frame, no loop.
    if (motion === 'reduced') { draw(0); return; }

    const ctrl = makeLoop(draw);

    const io = new IntersectionObserver(
      ([entry]) => {
        ctrl.setIntersecting(entry.isIntersecting);
        if (entry.isIntersecting) ctrl.startIfActive();
        else ctrl.stop();
      },
      { threshold: 0 }
    );
    io.observe(canvas);

    const onVisibility = () => {
      ctrl.setDocVisible(!document.hidden);
      if (!document.hidden) ctrl.startIfActive();
      else ctrl.stop();
    };
    document.addEventListener('visibilitychange', onVisibility);

    ctrl.startIfActive();

    return () => {
      ctrl.stop();
      io.disconnect();
      document.removeEventListener('visibilitychange', onVisibility);
    };
  }, [theme, motion]);
  return <canvas ref={canvasRef} aria-label="Ransomware monitor animation" />;
});
RansomwareMonitorAnimation.displayName = 'RansomwareMonitorAnimation';

// ───────────────────────────────────────────────────────────────────
// 4. Panic Keyword — typed-text matcher
// ───────────────────────────────────────────────────────────────────
//
// Shows a text input being typed into; the matcher hashes each token
// and compares against registered secret hashes; when a match hits,
// panic fires.
export const PanicKeywordAnimation = forwardRef<MonitorAnimationHandle, {}>((_, ref) => {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const overrideRef = useRef<number | null>(null);
  const { theme } = useTheme();
  const motion = useMotionPreference();

  useImperativeHandle(ref, () => ({
    fireAlarm(hold = 3) {
      overrideRef.current = performance.now() / 1000 + hold;
    },
  }), []);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const { ctx, w, h } = initCanvas(canvas);
    if (!ctx) return;
    const p = paletteFor(theme);
    const redGlow = makeGlow(255, 30, 30, 280);
    const accentGlow = makeGlow(0, 200, 255, 220);
    const SECRET = 'red dragon nine';
    const TYPED_CARRIER = 'hello world ';
    const FULL = TYPED_CARRIER + SECRET;
    const CYCLE = 10;

    const draw = (t: number) => {
      const tc = t % CYCLE;
      let phase: 0 | 1 | 2 | 3 = 0;
      let pp = 0;
      const now = performance.now() / 1000;
      const overridden = overrideRef.current !== null && now < overrideRef.current;
      if (overridden) {
        phase = 3; pp = 1;
      } else {
        if (overrideRef.current !== null && now >= overrideRef.current) overrideRef.current = null;
        if (tc < 1) { phase = 0; pp = tc; }
        else if (tc < 5.5) { phase = 1; pp = (tc - 1) / 4.5; }     // typing
        else if (tc < 6.5) { phase = 2; pp = (tc - 5.5) / 1; }     // hash-match
        else if (tc < 8.5) { phase = 3; pp = (tc - 6.5) / 2; }     // alarm
        else { phase = 0; pp = 0; }
      }

      ctx.fillStyle = p.bg; ctx.fillRect(0, 0, w, h);

      if (phase === 3) {
        ctx.globalAlpha = 0.7 + 0.3 * Math.sin(tc * 8);
        ctx.drawImage(redGlow, w / 2 - 140, h / 2 - 130);
        ctx.globalAlpha = 1;
      }

      // ── Text input box (centerpiece) ──
      const inpW = 380, inpH = 56;
      const inpX = w / 2 - inpW / 2;
      const inpY = h / 2 - inpH / 2 - 10;

      ctx.fillStyle = p.card;
      ctx.strokeStyle = phase === 3 ? p.danger : (phase === 2 ? p.accent : p.cardBorder);
      ctx.lineWidth = 2;
      roundRect(ctx, inpX, inpY, inpW, inpH, 8);
      ctx.fill();
      ctx.stroke();

      // Type the carrier + secret progressively in phase 1
      let typed = '';
      if (phase === 0) typed = '';
      else if (phase === 1) {
        const idx = Math.min(FULL.length, Math.floor(pp * FULL.length * 1.05));
        typed = FULL.slice(0, idx);
      } else {
        typed = FULL;
      }

      // Render the typed text with secret in different color when complete
      ctx.font = 'bold 14px ui-monospace, monospace';
      const carrierLen = TYPED_CARRIER.length;
      const showSecret = typed.length >= carrierLen;
      const carrierShown = typed.slice(0, Math.min(typed.length, carrierLen));
      const secretShown = typed.slice(carrierLen);

      ctx.fillStyle = p.textPrimary;
      ctx.fillText(carrierShown, inpX + 14, inpY + 35);
      const carrierW = ctx.measureText(carrierShown).width;

      if (showSecret) {
        ctx.fillStyle = phase === 3 ? p.danger : (phase === 2 ? p.accent : p.warn);
        ctx.fillText(secretShown, inpX + 14 + carrierW, inpY + 35);
      }

      // Blinking cursor
      const cursorBlink = Math.floor(t * 2) % 2 === 0;
      if (cursorBlink && phase !== 3) {
        const cx = inpX + 14 + ctx.measureText(typed).width + 1;
        ctx.fillStyle = p.textPrimary;
        ctx.fillRect(cx, inpY + 18, 2, 22);
      }

      // ── Hash matcher (below input) ──
      const hashY = inpY + inpH + 30;
      ctx.fillStyle = p.textMuted;
      ctx.font = '10px ui-monospace, monospace';
      ctx.textAlign = 'center';
      if (phase === 1) {
        ctx.fillText('LOCAL SHA-256 STREAM (zero data leaves the device)', w / 2, hashY);
      } else if (phase === 2 || phase === 3) {
        // Hash bytes box
        const hbW = 380, hbH = 38;
        const hbX = w / 2 - hbW / 2;
        ctx.fillStyle = p.card;
        ctx.strokeStyle = phase === 3 ? p.danger : p.accent;
        ctx.lineWidth = 1.5;
        roundRect(ctx, hbX, hashY - 6, hbW, hbH, 6);
        ctx.fill();
        ctx.stroke();
        if (phase === 2 && pp > 0.4) {
          ctx.drawImage(accentGlow, w / 2 - 110, hashY - 26);
        }
        ctx.fillStyle = phase === 3 ? p.danger : p.accent;
        ctx.font = 'bold 11px ui-monospace, monospace';
        ctx.fillText('sha256(red dragon nine) → 7c54f9a3b1d28e6f...', w / 2, hashY + 18);
      } else {
        ctx.fillText('waiting for input…', w / 2, hashY);
      }
      ctx.textAlign = 'left';

      // Banner
      ctx.textAlign = 'center';
      if (phase === 3) {
        ctx.fillStyle = `rgba(220, 38, 38, ${0.85 + 0.15 * Math.sin(tc * 10)})`;
        ctx.fillRect(0, 18, w, 38);
        ctx.fillStyle = '#fff';
        ctx.font = 'bold 13px ui-monospace, monospace';
        ctx.fillText('⚠  LOCKDOWN WORD MATCHED — LOCKDOWN FIRED  ⚠', w / 2, 42);
      } else {
        const status = phase === 2 ? 'HASH MATCH FOUND' : phase === 1 ? 'INTERCEPTING KEYSTROKES' : 'LISTENING FOR LOCKDOWN WORD';
        ctx.fillStyle = phase === 2 ? p.accent : p.textMuted;
        ctx.font = 'bold 11px ui-monospace, monospace';
        ctx.fillText(status, w / 2, 32);
      }
      ctx.textAlign = 'left';
    };

    // Reduced-motion: one static idle frame, no loop.
    if (motion === 'reduced') { draw(0); return; }

    const ctrl = makeLoop(draw);

    const io = new IntersectionObserver(
      ([entry]) => {
        ctrl.setIntersecting(entry.isIntersecting);
        if (entry.isIntersecting) ctrl.startIfActive();
        else ctrl.stop();
      },
      { threshold: 0 }
    );
    io.observe(canvas);

    const onVisibility = () => {
      ctrl.setDocVisible(!document.hidden);
      if (!document.hidden) ctrl.startIfActive();
      else ctrl.stop();
    };
    document.addEventListener('visibilitychange', onVisibility);

    ctrl.startIfActive();

    return () => {
      ctrl.stop();
      io.disconnect();
      document.removeEventListener('visibilitychange', onVisibility);
    };
  }, [theme, motion]);
  return <canvas ref={canvasRef} aria-label="Lockdown word animation" />;
});
PanicKeywordAnimation.displayName = 'PanicKeywordAnimation';
