// src/utils/sound.ts
//
// Sound utility — synthesizes short mechanical feedback sounds via Web Audio API.
// No audio files required. Respects settings.app.sounds.enabled (default: true).
// All sounds <200ms, volume ≤ 0.3. Never throws.

type SoundName = 'toggle' | 'complete' | 'threshold' | 'warning';

let soundEnabled = true;

// ── AudioContext singleton ────────────────────────────────────────────────────

let _ctx: AudioContext | null = null;

function getCtx(): AudioContext | null {
    try {
        if (!_ctx) _ctx = new AudioContext();
        if (_ctx.state === 'suspended') void _ctx.resume();
        return _ctx;
    } catch { return null; }
}

// ── Settings fast-path ───────────────────────────────────────────────────────

function isSoundEnabled(): boolean {
    return soundEnabled;
}

export function isSoundPlaybackEnabled(): boolean {
    return isSoundEnabled();
}

/** Call this when settings.app.sounds.enabled changes */
export function setSoundEnabled(enabled: boolean): void {
    soundEnabled = enabled;
}

// ── Synthesizers ─────────────────────────────────────────────────────────────

/** Toggle: white-noise burst through high-pass → electrical relay click (~35ms) */
function synth_toggle(ctx: AudioContext): void {
    const n = Math.floor(ctx.sampleRate * 0.035);
    const buf = ctx.createBuffer(1, n, ctx.sampleRate);
    const d = buf.getChannelData(0);
    for (let i = 0; i < n; i++) d[i] = (Math.random() * 2 - 1) * Math.exp(-7 * i / n);
    const src = ctx.createBufferSource(); src.buffer = buf;
    const hpf = ctx.createBiquadFilter(); hpf.type = 'highpass'; hpf.frequency.value = 1600;
    const g = ctx.createGain(); g.gain.value = 0.22;
    src.connect(hpf); hpf.connect(g); g.connect(ctx.destination);
    src.start();
}

/** Complete: descending sine → submarine hatch closing (~110ms) */
function synth_complete(ctx: AudioContext): void {
    const t = ctx.currentTime;
    const osc = ctx.createOscillator(); osc.type = 'sine';
    osc.frequency.value = 460; osc.frequency.linearRampToValueAtTime(180, t + 0.1);
    const g = ctx.createGain(); g.gain.value = 0;
    g.gain.linearRampToValueAtTime(0.28, t + 0.01);
    g.gain.linearRampToValueAtTime(0, t + 0.13);
    osc.connect(g); g.connect(ctx.destination);
    osc.start(t); osc.stop(t + 0.15);
}

/** Threshold: ascending authority tone → system lock engaging (~160ms) */
function synth_threshold(ctx: AudioContext): void {
    const t = ctx.currentTime;
    const osc = ctx.createOscillator(); osc.type = 'sine';
    osc.frequency.value = 300; osc.frequency.linearRampToValueAtTime(540, t + 0.09);
    const g = ctx.createGain(); g.gain.value = 0;
    g.gain.linearRampToValueAtTime(0.28, t + 0.01);
    g.gain.linearRampToValueAtTime(0, t + 0.18);
    osc.connect(g); g.connect(ctx.destination);
    osc.start(t); osc.stop(t + 0.2);
}

/** Warning: two sharp sine pings (~100ms total) */
function synth_warning(ctx: AudioContext): void {
    const ping = (delay: number) => {
        const t = ctx.currentTime + delay;
        const osc = ctx.createOscillator(); osc.type = 'sine'; osc.frequency.value = 880;
        const g = ctx.createGain(); g.gain.value = 0;
        g.gain.linearRampToValueAtTime(0.22, t + 0.005);
        g.gain.linearRampToValueAtTime(0, t + 0.04);
        osc.connect(g); g.connect(ctx.destination);
        osc.start(t); osc.stop(t + 0.05);
    };
    ping(0); ping(0.065);
}

// ── Public API ────────────────────────────────────────────────────────────────

export function playSound(name: SoundName): void {
    if (!isSoundEnabled()) return;
    const ctx = getCtx();
    if (!ctx) return;
    try {
        switch (name) {
            case 'toggle':    synth_toggle(ctx);    break;
            case 'complete':  synth_complete(ctx);  break;
            case 'threshold': synth_threshold(ctx); break;
            case 'warning':   synth_warning(ctx);   break;
        }
    } catch { /* ignore */ }
}
