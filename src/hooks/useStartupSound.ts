import { useCallback } from 'react';
import { isSoundPlaybackEnabled } from '../utils/sound';

export const useStartupSound = () => {
    const playStartupSound = useCallback(() => {
        try {
            if (!isSoundPlaybackEnabled()) return;

            const AudioContext = window.AudioContext || (window as any).webkitAudioContext;
            if (!AudioContext) return;

            const ctx = new AudioContext();

            // Create a master gain node for overall volume control
            const masterGain = ctx.createGain();
            masterGain.connect(ctx.destination);
            masterGain.gain.setValueAtTime(0.15, ctx.currentTime); // Keep it subtle

            // F Major 7th chord (F4, A4, C5, E5) - Classic, airy, positive
            const notes = [349.23, 440.00, 523.25, 659.25];

            notes.forEach((freq, index) => {
                const osc = ctx.createOscillator();
                const gain = ctx.createGain();

                // Use sine for checking pure tone, triangle for a bit more body
                osc.type = index % 2 === 0 ? 'sine' : 'triangle';
                osc.frequency.setValueAtTime(freq, ctx.currentTime);

                // Slight detune for richness
                const detune = (Math.random() - 0.5) * 4;
                osc.detune.setValueAtTime(detune, ctx.currentTime);

                // Envelope
                const now = ctx.currentTime;
                const attack = 0.05;
                const decay = 2.5;

                // Stagger entries slightly for a "strum" or "bloom" effect
                const startOffset = index * 0.04;

                gain.gain.setValueAtTime(0, now);
                gain.gain.linearRampToValueAtTime(0.3, now + startOffset + attack);
                gain.gain.exponentialRampToValueAtTime(0.001, now + startOffset + attack + decay);

                osc.connect(gain);
                gain.connect(masterGain);

                osc.start(now + startOffset);
                osc.stop(now + startOffset + attack + decay + 0.1);
            });

            // Cleanup context after sound finishes
            setTimeout(() => {
                ctx.close();
            }, 4000);

        } catch (e) {
            console.warn("Failed to play startup sound", e);
        }
    }, []);

    return { playStartupSound };
};
