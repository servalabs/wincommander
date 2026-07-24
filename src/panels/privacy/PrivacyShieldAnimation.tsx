import { useRef, useEffect } from 'react';
import useMotionPreference from '@/hooks/useMotionPreference';

// ── Shared Utilities & Constants ───────────────────────────────────────
const ANIM_H = 550;
const BG = '#e8f0f7';
const FRAME_MS = 1000 / 30; // Capped at 30fps

const containerStyle = {
  width: '100%',
  height: '100%',
  maxHeight: '450px',
  borderRadius: 12,
  overflow: 'hidden',
  background: BG,
  display: 'flex',
  justifyContent: 'center',
  alignItems: 'center'
};

function initCanvas(c: HTMLCanvasElement) {
  try {
    const w = 600;
    const h = ANIM_H;
    const d = Math.min(window.devicePixelRatio ?? 1, 2);
    c.width = w * d;
    c.height = h * d;
    c.style.cssText = `display:block;max-width:100%;max-height:100%;object-fit:contain;`;
    const ctx = c.getContext('2d');
    if (!ctx) return { ctx: null, w, h };
    ctx.scale(d, d);
    return { ctx, w, h };
  } catch {
    return { ctx: null, w: 0, h: 0 };
  }
}

function makeGlow(r: number, g: number, b: number, sz = 250) {
  const c = document.createElement('canvas');
  c.width = c.height = sz;
  const x = c.getContext('2d');
  if (!x) return c;
  const half = sz / 2;
  const gr = x.createRadialGradient(half, half, 0, half, half, half);
  gr.addColorStop(0, `rgba(${r},${g},${b},0.55)`);
  gr.addColorStop(0.35, `rgba(${r},${g},${b},0.12)`);
  gr.addColorStop(1, 'transparent');
  x.fillStyle = gr;
  x.fillRect(0, 0, sz, sz);
  return c;
}

// ── The Privacy Shield Animation ──────────────
export default function PrivacyShieldAnimation() {
  const ref = useRef<HTMLCanvasElement>(null);
  const motion = useMotionPreference();

  useEffect(() => {
    const canvas = ref.current;
    if (!canvas) return;
    const { ctx, w, h } = initCanvas(canvas);
    if (!ctx) return;

    const cx = w / 2, cy = h / 2 - 20;
    const redGlow = makeGlow(255, 30, 30, 250);
    const amberGlow = makeGlow(255, 170, 0, 250);
    const orangeGlow = makeGlow(255, 120, 0, 250);

    const lapW = 320, lapH = 220;
    const scrX = cx - lapW / 2 + 12, scrY = cy - lapH / 2 + 8;
    const scrW = lapW - 24, scrH = lapH - 35;

    // Restored laptop screen content
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

    // Draw the idle "all-clear" frame — used as static placeholder when motion
    // is suppressed (reduced-motion preference or canvas off-screen / tab hidden).
    const drawStaticFrame = () => {
      ctx.fillStyle = BG;
      ctx.fillRect(0, 0, w, h);
      ctx.save();
      ctx.fillStyle = '#d4dce5';
      ctx.strokeStyle = 'rgba(100,120,150,0.6)';
      ctx.lineWidth = 2;
      ctx.beginPath();
      if (ctx.roundRect) ctx.roundRect(cx - lapW / 2, cy - lapH / 2, lapW, lapH - 20, 8);
      else ctx.rect(cx - lapW / 2, cy - lapH / 2, lapW, lapH - 20);
      ctx.fill();
      ctx.stroke();
      ctx.beginPath();
      ctx.arc(cx, cy - lapH / 2 + 5, 3, 0, Math.PI * 2);
      ctx.fillStyle = 'rgba(0,150,200,0.6)';
      ctx.fill();
      const lapBaseY = cy - lapH / 2 + lapH - 20;
      ctx.fillStyle = '#c0cad6';
      ctx.beginPath();
      ctx.moveTo(cx - lapW / 2 - 12, lapBaseY);
      ctx.lineTo(cx + lapW / 2 + 12, lapBaseY);
      ctx.lineTo(cx + lapW / 2 + 24, lapBaseY + 20);
      ctx.lineTo(cx - lapW / 2 - 24, lapBaseY + 20);
      ctx.closePath();
      ctx.fill();
      ctx.stroke();
      ctx.save();
      ctx.beginPath();
      ctx.rect(scrX, scrY, scrW, scrH);
      ctx.clip();
      ctx.fillStyle = '#ffffff';
      ctx.fillRect(scrX, scrY, scrW, scrH);
      ctx.font = '10px "Courier New", monospace';
      ctx.fillStyle = '#000000';
      ctx.textBaseline = 'top';
      let cursorY = scrY + 10;
      const textX = scrX + 10;
      for (let i = 0; i < codeLines.length; i++) {
        ctx.fillText(codeLines[i], textX, cursorY);
        cursorY += 13;
      }
      ctx.restore();
      ctx.restore();
    };

    // Reduced-motion: render one static frame and stop — no loop needed.
    if (motion === 'reduced') {
      drawStaticFrame();
      return;
    }

    const CYCLE = 16.0;
    const startTime = performance.now() / 1000;

    let raf = 0;
    let lastTs = 0;
    // Visibility gates — loop is live only when both are true.
    let isIntersecting = true;
    let isDocVisible = !document.hidden;

    const loop = (ts: number) => {
      if (!ctx || ts - lastTs < FRAME_MS) { raf = requestAnimationFrame(loop); return; }
      lastTs = ts;
      const now = performance.now() / 1000;
      const t = now - startTime;
      const timeInCycle = t % CYCLE;

      const easeOut = (val: number) => 1 - Math.pow(1 - val, 3);

      // Security Phases
      let gazeP = 0, phoneP = 0, multiPersonP = 0;

      // Gaze Lost
      if (timeInCycle >= 1 && timeInCycle < 1.5) gazeP = (timeInCycle - 1) / 0.5;
      else if (timeInCycle >= 1.5 && timeInCycle < 3.5) gazeP = 1;
      else if (timeInCycle >= 3.5 && timeInCycle < 4.0) gazeP = 1 - (timeInCycle - 3.5) / 0.5;

      // Phone Detected
      if (timeInCycle >= 6 && timeInCycle < 6.5) phoneP = (timeInCycle - 6) / 0.5;
      else if (timeInCycle >= 6.5 && timeInCycle < 8.5) phoneP = 1;
      else if (timeInCycle >= 8.5 && timeInCycle < 9.0) phoneP = 1 - (timeInCycle - 8.5) / 0.5;

      // Multiple Persons
      if (timeInCycle >= 11 && timeInCycle < 11.5) multiPersonP = (timeInCycle - 11) / 0.5;
      else if (timeInCycle >= 11.5 && timeInCycle < 13.5) multiPersonP = 1;
      else if (timeInCycle >= 13.5 && timeInCycle < 14.0) multiPersonP = 1 - (timeInCycle - 13.5) / 0.5;

      gazeP = easeOut(gazeP);
      phoneP = easeOut(phoneP);
      multiPersonP = easeOut(multiPersonP);

      let alertText = null;
      let alertColor = '';
      let blurAmount = 0;

      if (gazeP > 0.5) { alertText = '⚠ GAZE LOST ⚠'; alertColor = '#febc2e'; blurAmount = 8; }
      else if (phoneP > 0.5) { alertText = '⚠ PHONE DETECTED ⚠'; alertColor = '#ff2222'; blurAmount = 8; }
      else if (multiPersonP > 0.5) { alertText = '⚠ MULTIPLE PERSONS ⚠'; alertColor = '#ff8822'; blurAmount = 8; }

      // Background
      ctx.fillStyle = BG;
      ctx.fillRect(0, 0, w, h);

      ctx.save();

      // ─── 1. Laptop Shell ───
      ctx.fillStyle = '#d4dce5';
      ctx.strokeStyle = alertText ? alertColor : 'rgba(100,120,150,0.6)';
      ctx.lineWidth = 2;
      ctx.beginPath();
      if (ctx.roundRect) ctx.roundRect(cx - lapW / 2, cy - lapH / 2, lapW, lapH - 20, 8);
      else ctx.rect(cx - lapW / 2, cy - lapH / 2, lapW, lapH - 20);
      ctx.fill();
      ctx.stroke();

      const webcamY = cy - lapH / 2 + 5;
      ctx.beginPath();
      ctx.arc(cx, webcamY, 3, 0, Math.PI * 2);
      ctx.fillStyle = alertText ? alertColor : 'rgba(0,150,200,0.6)';
      ctx.fill();

      // Laptop Base
      const lapBaseY = cy - lapH / 2 + lapH - 20;
      ctx.fillStyle = '#c0cad6';
      ctx.beginPath();
      ctx.moveTo(cx - lapW / 2 - 12, lapBaseY);
      ctx.lineTo(cx + lapW / 2 + 12, lapBaseY);
      ctx.lineTo(cx + lapW / 2 + 24, lapBaseY + 20);
      ctx.lineTo(cx - lapW / 2 - 24, lapBaseY + 20);
      ctx.closePath();
      ctx.fill();
      ctx.stroke();

      // ─── 2. Restored Screen Content & Blur ───
      ctx.save();
      ctx.beginPath();
      ctx.rect(scrX, scrY, scrW, scrH);
      ctx.clip();

      // White background
      ctx.fillStyle = '#ffffff';
      ctx.fillRect(scrX, scrY, scrW, scrH);

      // Only show text when NOT blurred
      if (blurAmount === 0) {
        ctx.font = '10px "Courier New", monospace';
        ctx.fillStyle = '#000000';
        ctx.textBaseline = 'top';

        let cursorY = scrY + 10;
        const textX = scrX + 10;
        const lineHeight = 13;

        for (let i = 0; i < codeLines.length; i++) {
          ctx.fillText(codeLines[i], textX, cursorY);
          cursorY += lineHeight;
        }
      } else {
        // Apply blur
        ctx.filter = `blur(${blurAmount}px)`;
        ctx.fillStyle = '#ffffff';
        ctx.fillRect(scrX, scrY, scrW, scrH);

        // Redraw slightly blurred text lines so the blur looks realistic
        ctx.fillStyle = 'rgba(0,0,0,0.2)';
        let cursorY = scrY + 10;
        const textX = scrX + 10;
        for (let i = 0; i < codeLines.length; i++) {
          ctx.fillText(codeLines[i], textX, cursorY);
          cursorY += 13;
        }
      }
      ctx.restore();

      // ─── 3. Alert Center Overlay ───
      if (alertText) {
        ctx.fillStyle = 'rgba(10,0,0,0.85)';
        ctx.beginPath();
        if (ctx.roundRect) ctx.roundRect(cx - 120, cy - 16, 240, 32, 6);
        else ctx.rect(cx - 120, cy - 16, 240, 32);
        ctx.fill();
        ctx.strokeStyle = alertColor;
        ctx.lineWidth = 1.5;
        ctx.stroke();

        ctx.fillStyle = alertColor;
        ctx.font = 'bold 12px sans-serif';
        ctx.textAlign = 'center';
        ctx.textBaseline = 'middle';
        ctx.fillText(alertText, cx, cy);

        ctx.globalAlpha = 0.5;
        const currentGlow = alertColor === '#ff2222' ? redGlow :
                           alertColor === '#ff8822' ? orangeGlow : amberGlow;
        ctx.drawImage(currentGlow, cx - 125, cy - 125, 250, 250);
        ctx.globalAlpha = 1;
      }
      ctx.restore();

      // ─── 4. Barebones Webcam Preview (Cleaned up) ───
      const camW = 200, camH = 150;
      const camX = cx - camW / 2;
      const camYPreview = cy + lapH / 2 - 120;

      ctx.save();
      ctx.beginPath();
      if(ctx.roundRect) ctx.roundRect(camX, camYPreview, camW, camH, 10);
      else ctx.rect(camX, camYPreview, camW, camH);
      ctx.clip();

      // NO background - transparent, blends with main background

      // Render Silhouettes inside the webcam preview
      const personCount = multiPersonP > 0.5 ? 3 : 1;
      const spacing = 55;
      const startX = camX + camW / 2 - ((personCount - 1) * spacing) / 2;
      const personBaseY = camYPreview + camH - 15;

      for (let i = 0; i < personCount; i++) {
        const personX = startX + (i * spacing);
        const personColor = gazeP > 0.5 ? 'rgba(255, 188, 46, 0.9)' :
                            multiPersonP > 0.5 ? 'rgba(255, 136, 34, 0.9)' : 'rgba(0, 242, 255, 0.9)';
        ctx.fillStyle = personColor;

        ctx.beginPath();
        ctx.ellipse(personX, personBaseY, 42, 30, 0, 0, Math.PI * 2);
        ctx.fill();
        ctx.fillRect(personX - 10, personBaseY - 30, 20, 18);

        const headShift = (i === 0 && gazeP > 0.5) ? gazeP * 15 : 0;
        ctx.beginPath();
        ctx.arc(personX + headShift, personBaseY - 42, 22, 0, Math.PI * 2);
        ctx.fill();

        ctx.fillStyle = '#1a1a1a';
        const eyeShift = (i === 0 && gazeP > 0.5) ? gazeP * 8 : 0;
        ctx.beginPath(); ctx.arc(personX - 7 + headShift + eyeShift, personBaseY - 44, 3.5, 0, Math.PI * 2); ctx.fill();
        ctx.beginPath(); ctx.arc(personX + 7 + headShift + eyeShift, personBaseY - 44, 3.5, 0, Math.PI * 2); ctx.fill();
        ctx.strokeStyle = '#1a1a1a';
        ctx.lineWidth = 2;
        ctx.beginPath(); ctx.arc(personX + headShift, personBaseY - 36, 5, 0, Math.PI); ctx.stroke();
      }

      // Render tiny phone INSIDE webcam preview
      if (phoneP > 0.5) {
        const pX = camX + 30, pY = camYPreview + camH - 20 - (phoneP * 40);
        ctx.fillStyle = gazeP > 0.5 ? 'rgba(255, 188, 46, 0.7)' : 'rgba(0, 242, 255, 0.7)';
        ctx.beginPath(); ctx.ellipse(pX + 11, pY + 45, 15, 10, 0, 0, Math.PI * 2); ctx.fill();
        ctx.fillStyle = '#1a1a1a';
        ctx.beginPath(); ctx.roundRect ? ctx.roundRect(pX, pY, 22, 40, 3) : ctx.rect(pX, pY, 22, 40); ctx.fill();
        ctx.strokeStyle = '#ff2222'; ctx.lineWidth = 2; ctx.stroke();
        ctx.fillStyle = '#474545'; ctx.fillRect(pX + 2, pY + 4, 18, 32);
      }
      ctx.restore();

      raf = requestAnimationFrame(loop);
    };

    // Start the loop only when canvas is intersecting AND tab is visible.
    const startIfActive = () => {
      if (isIntersecting && isDocVisible && raf === 0) {
        raf = requestAnimationFrame(loop);
      }
    };

    const stopLoop = () => {
      if (raf !== 0) { cancelAnimationFrame(raf); raf = 0; }
    };

    // Pause when scrolled off-screen; resume when back in view.
    const io = new IntersectionObserver(
      ([entry]) => {
        isIntersecting = entry.isIntersecting;
        if (isIntersecting) { startIfActive(); }
        else { stopLoop(); drawStaticFrame(); }
      },
      { threshold: 0 }
    );
    io.observe(canvas);

    // Pause when the tab/window is hidden; resume when visible again.
    const onVisibility = () => {
      isDocVisible = !document.hidden;
      if (isDocVisible) { startIfActive(); }
      else { stopLoop(); }
    };
    document.addEventListener('visibilitychange', onVisibility);

    startIfActive();

    return () => {
      stopLoop();
      io.disconnect();
      document.removeEventListener('visibilitychange', onVisibility);
    };
  }, [motion]);

  return <div style={containerStyle}><canvas ref={ref} /></div>;
}