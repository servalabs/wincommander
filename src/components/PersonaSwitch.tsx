// src/components/PersonaSwitch.tsx
//
// Settings control for the threat-model persona axis (spec section 4).
// Sibling to ExperienceLevelSwitch.tsx, but orthogonal: persona ("casual" |
// "secure") drives which coarse feature modules default on, not UI density.
// Switching persona re-seeds ONLY the three persona-controlled modules
// (cleanup/flows/vault) via modulesForPersona — other modules the user set
// are left alone. Gated behind a confirm dialog since it changes state.

import { Icon } from "@/components/ui/icon";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { useState } from "react";
import { useAppState } from "../context/AppContext";
import { getPersona, type ThreatPersona } from "../types/settings";
import { modulesForPersona, PERSONA_CONTROLLED_MODULES, type ModuleConfig } from "../types/modules";

// patchAppSettings merges app.modules key-by-key over the user's current config,
// so we send ONLY the persona-controlled ids — sending the full modulesForPersona()
// map would reset every OTHER module (network, privacy, mesh, …) to advanced
// defaults. The id list lives in modules.ts to stay in sync with the helper.

const PERSONAS: { value: ThreatPersona; label: string; icon: "user" | "shield" }[] = [
  { value: "casual", label: "Casual", icon: "user" },
  { value: "secure", label: "Secure", icon: "shield" },
];

export default function PersonaSwitch({ compact = false }: { compact?: boolean }) {
  const { patchAppSettings, appSettings } = useAppState();
  const currentPersona = getPersona(appSettings);
  const [pendingPersona, setPendingPersona] = useState<ThreatPersona | null>(null);

  const requestPersonaChange = (next: ThreatPersona) => {
    if (next === currentPersona) return;
    setPendingPersona(next);
  };

  const confirmPersonaChange = async () => {
    if (!pendingPersona) return;
    const next = pendingPersona;
    setPendingPersona(null);
    try {
      const seeded = modulesForPersona(next);
      const personaModules: ModuleConfig = {};
      for (const id of PERSONA_CONTROLLED_MODULES) {
        personaModules[id] = seeded[id];
      }
      await patchAppSettings({
        app: {
          persona: next,
          modules: personaModules,
        },
      });
    } catch (err) {
      console.error('Failed to change persona:', err);
    }
  };

  const current = PERSONAS.find((p) => p.value === currentPersona);

  const content = (
    <>
      <div className={`exp-header ${compact ? 'exp-header--compact' : ''}`}>
        <span className="exp-label">PERSONA</span>
        <span className="exp-level-badge">
          {current?.label.toUpperCase()}
        </span>
      </div>
      <div className={`exp-track ${compact ? 'exp-track--compact' : ''}`}>
        {PERSONAS.map((persona) => (
          <button
            key={persona.value}
            className={`exp-btn ${compact ? 'exp-btn--compact' : ''} ${currentPersona === persona.value ? 'active' : ''}`}
            onClick={() => requestPersonaChange(persona.value)}
            title={persona.label}
          >
            <Icon className="exp-icon" icon={persona.icon} size={compact ? 12 : 13} />
            <span className={`exp-btn-label ${compact ? 'exp-btn-label--compact' : ''}`}>{persona.label}</span>
          </button>
        ))}
      </div>
    </>
  );

  return (
    <>
      {compact ? <div className="exp-col">{content}</div> : <div className="experience-switch">{content}</div>}

      <AlertDialog open={pendingPersona !== null} onOpenChange={(open) => { if (!open) setPendingPersona(null); }}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Switch to {pendingPersona === "casual" ? "Casual" : "Secure"}?</AlertDialogTitle>
            <AlertDialogDescription>
              {pendingPersona === "casual"
                ? "Casual turns off System Cleanup, Flows, and Encrypted Volumes — you can re-enable any of them anytime."
                : "Secure turns on System Cleanup, Flows, and Encrypted Volumes — you can turn off any of them anytime."}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction onClick={confirmPersonaChange}>
              Switch
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  );
}
