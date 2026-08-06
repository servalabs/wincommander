import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  CommandDialog,
  CommandInput,
  CommandList,
  CommandEmpty,
  CommandGroup,
  CommandItem,
  CommandSeparator,
  CommandFooter,
} from "./ui/command";
import { Icon } from "./ui/icon";
import { getSidebarManifests, PANEL_MANIFESTS, type PanelId } from "../types/panels";
import { ALL_TOGGLES } from "../registry";
import { getToggleVisibility, resolveToggleText, type ToggleDef } from "../types/toggles";
import useVisibility from "../hooks/useVisibility";
import useBorrowedActive from "../hooks/useBorrowedActive";
import { useSearchQuery } from "../context/SearchContext";
import { useTheme } from "../context/ThemeContext";
import { useAppState } from "../context/AppContext";
import { useAuthMode } from "../context/AuthModeContext";
import { cn } from "../lib/utils";
import { DEFAULT_BORROWED_PANELS } from "../lib/visibilityDefaults";

const PANEL_LABEL: Partial<Record<string, string>> = Object.fromEntries(
  PANEL_MANIFESTS.map((p) => [p.id, p.label])
);

// Toggle domain → the panel that hosts it (mirrors GlobalSearchNoResults).
const DOMAIN_PANEL: Record<ToggleDef["domain"], PanelId> = {
  privacy: "privacy",
  tweaks: "tweaks",
  network: "network",
  identity: "system-identity",
  security: "tweaks",
};

// Panel → badge color classes (text + bg).
const PANEL_BADGE: Partial<Record<string, string>> = {
  privacy:           "bg-[var(--accent-soft)] text-[var(--accent-2)]",
  network:           "bg-[rgba(121,201,143,0.12)] text-[var(--ok)]",
  tweaks:            "bg-[rgba(245,180,84,0.12)] text-[var(--warn)]",
  "system-identity": "bg-[rgba(111,176,230,0.12)] text-[var(--color-info)]",
};

const ICON_TILE = "flex h-7 w-7 shrink-0 items-center justify-center rounded-[var(--r-sm)] bg-[var(--surface-3)] transition-colors duration-150 group-data-[selected=true]:bg-[var(--accent-soft)]";
const ICON_CLS  = "text-[var(--text-mute)] transition-colors duration-150 group-data-[selected=true]:text-[var(--accent)]";

const KBD_HINTS: [string, string][] = [["↑↓", "navigate"], ["⏎", "select"], ["esc", "close"]];

/**
 * GlobalCommandPalette — the unified ⌘K finder.
 * Searches navigation, settings/toggles, quick actions and file search in one
 * surface. Opened by Ctrl/⌘+K or the titlebar search trigger
 * (`open-command-palette` event).
 */
export default function GlobalCommandPalette() {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [panelsUnlocked, setPanelsUnlocked] = useState(false);
  const [secretSettingsRevealed, setSecretSettingsRevealed] = useState(false);
  const visibility = useVisibility();
  const borrowedActive = useBorrowedActive();
  const { setSearchQuery } = useSearchQuery();
  const { theme, setTheme } = useTheme();
  const { appSettings } = useAppState();
  const { setMode } = useAuthMode();

  // Secret unlock/lock keyword — moved here from the (now-removed) per-panel
  // search bar. Typing the user's keyword and pressing Enter fires the
  // hidden-panels event silently (no visible suggestion, preserving secrecy).
  const handleInputKeyDown = useCallback((e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key !== "Enter") return;
    const cmd = query.trim().toLowerCase();
    if (!cmd) return;
    const unlockWord = (appSettings?.app?.unlockKeyword || "unlock").toLowerCase();
    const lockWord = (appSettings?.app?.lockKeyword || "lock").toLowerCase();
    if (cmd === unlockWord || cmd === lockWord) {
      e.preventDefault();
      e.stopPropagation();
      window.dispatchEvent(new Event(cmd === unlockWord ? "hidden-panels-unlock" : "hidden-panels-lock"));
      setQuery("");
      setOpen(false);
    }

    // Distress phrase check — method D. Async verify so it doesn't block
    // the keydown handler. Non-matches return null and are invisible.
    if (cmd.length >= 3) {
      invoke<string | null>("check_distress_phrase", { phrase: cmd })
        .then((mode) => {
          if (mode === "decoy") {
            setQuery("");
            setOpen(false);
            setMode("decoy");
          } else if (mode === "destroy") {
            setQuery("");
            setOpen(false);
            invoke("lockdown", { deactivateLicenseFirst: false, shutdownSystem: false }).catch(() => {});
          }
        })
        .catch(() => {});
    }
  }, [query, appSettings?.app?.unlockKeyword, appSettings?.app?.lockKeyword, setMode]);

  // ── Track panel unlock state (mirrors App.tsx) ──
  useEffect(() => {
    const onUnlock = () => setPanelsUnlocked(true);
    const onLock = () => {
      setPanelsUnlocked(false);
      setSecretSettingsRevealed(false);
    };
    const onRevealSecret = () => setSecretSettingsRevealed(true);
    window.addEventListener("hidden-panels-unlock", onUnlock);
    window.addEventListener("hidden-panels-lock", onLock);
    window.addEventListener("secret-settings-reveal", onRevealSecret);
    return () => {
      window.removeEventListener("hidden-panels-unlock", onUnlock);
      window.removeEventListener("hidden-panels-lock", onLock);
      window.removeEventListener("secret-settings-reveal", onRevealSecret);
    };
  }, []);

  // ── Open triggers: Ctrl/⌘+K + the titlebar event ──
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && (e.key === "k" || e.key === "K")) {
        e.preventDefault();
        setOpen((o) => !o);
      }
    };
    const onOpen = () => setOpen(true);
    window.addEventListener("keydown", onKey);
    window.addEventListener("open-command-palette", onOpen);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("open-command-palette", onOpen);
    };
  }, []);

  // ── Auto-focus input on open (rAF needed for Tauri WebView2) ──
  useEffect(() => {
    if (!open) { setQuery(""); return; }
    const id = requestAnimationFrame(() => {
      document.querySelector<HTMLInputElement>('[data-slot="command-input"]')?.focus();
    });
    return () => cancelAnimationFrame(id);
  }, [open]);

  const navigate = useCallback((id: PanelId) => {
    setOpen(false);
    window.dispatchEvent(new CustomEvent("navigate-panel", { detail: id }));
  }, []);

  const fire = useCallback((event: string) => {
    setOpen(false);
    window.dispatchEvent(new CustomEvent(event));
  }, []);

  const level = visibility.density === "expert" ? "advanced" : "standard";

  // Panels that are currently locked (hidden until unlock keyword / 4-click).
  // KT: When borrowed and lockedPanelIds is unset (fresh install), fall back to
  // DEFAULT_BORROWED_PANELS so the palette hides the same panels the sidebar does.
  const lockedIds = useMemo(
    () => new Set<string>(panelsUnlocked ? [] : (borrowedActive
      ? (appSettings?.app?.lockedPanelIds ?? DEFAULT_BORROWED_PANELS)
      : (appSettings?.app?.lockedPanelIds ?? [])
    )),
    [panelsUnlocked, borrowedActive, appSettings?.app?.lockedPanelIds]
  );

  // Visible nav destinations — exclude locked panels.
  // Secret Settings is reveal-gated only (same rule as Sidebar) so Borrowed
  // Mode never strands the user without a way back into the panel.
  const panels = useMemo(
    () =>
      getSidebarManifests(visibility).filter((p) =>
        p.id === "secret" ? secretSettingsRevealed : !lockedIds.has(p.id),
      ),
    [visibility, lockedIds, secretSettingsRevealed],
  );
  const canShowPanel = useCallback(
    (id: PanelId) => (id === "secret" ? secretSettingsRevealed : !lockedIds.has(id)),
    [lockedIds, secretSettingsRevealed],
  );

  // Settings/toggles — exclude those belonging to a locked panel.
  const settings = useMemo(() => {
    return ALL_TOGGLES
      .filter((t) => visibility.isVisible(getToggleVisibility(t, visibility.profiles)))
      .filter((t) => !lockedIds.has(DOMAIN_PANEL[t.domain]))
      .map((t) => {
        const wording = resolveToggleText(t, level);
        return {
          id: t.id,
          label: wording.label,
          panel: DOMAIN_PANEL[t.domain],
          icon: (t.icon as string) ?? "cog",
          keywords: [...(t.keywords ?? []), t.domain, wording.label],
        };
      });
  }, [visibility, level, lockedIds]);

  const goSetting = useCallback((s: { label: string; panel: PanelId }) => {
    setOpen(false);
    // Pre-seed the panel filter so searchable panels (privacy/tweaks) land on it.
    setSearchQuery(s.label);
    window.dispatchEvent(new CustomEvent("navigate-panel", { detail: s.panel }));
  }, [setSearchQuery]);

  return (
    <CommandDialog open={open} onOpenChange={setOpen} title="Command palette" description="Search settings, files and actions">
      <CommandInput placeholder="Search settings, files & actions…" value={query} onValueChange={setQuery} onKeyDown={handleInputKeyDown} />
      <CommandList>
        <CommandEmpty>No matches.</CommandEmpty>

        <CommandGroup heading="Navigate">
          {panels.map((p) => (
            <CommandItem key={p.id} value={`go ${p.label}`} keywords={p.searchKeywords} onSelect={() => navigate(p.id)}>
              <div className={ICON_TILE}>
                <Icon icon={p.icon as string} size={13} className={ICON_CLS} />
              </div>
              <span>{p.label}</span>
            </CommandItem>
          ))}
          {canShowPanel("advisor") && (
            <CommandItem value="ai advisor assistant" keywords={["ai", "advisor", "llm", "assistant", "explain"]} onSelect={() => navigate("advisor")}>
              <div className={ICON_TILE}>
                <Icon icon="predictive-analysis" size={13} className={ICON_CLS} />
              </div>
              <span>AI Advisor</span>
            </CommandItem>
          )}
          {canShowPanel("search-files") && (
            <CommandItem value="search files everything" keywords={["files", "everything", "find", "locate"]} onSelect={() => navigate("search-files")}>
              <div className={ICON_TILE}>
                <Icon icon="search" size={13} className={ICON_CLS} />
              </div>
              <span>Search files…</span>
            </CommandItem>
          )}
        </CommandGroup>

        <CommandSeparator />

        <CommandGroup heading="Settings">
          {settings.map((s) => (
            <CommandItem key={`set-${s.id}`} value={`setting ${s.label} ${s.id}`} keywords={s.keywords} onSelect={() => goSetting(s)}>
              <div className={ICON_TILE}>
                <Icon icon={s.icon} size={13} className={ICON_CLS} />
              </div>
              <span className="capitalize">{s.label.toLowerCase()}</span>
              <span className={cn(
                "ml-auto shrink-0 rounded px-1.5 py-0.5 text-[10px] font-medium font-[family-name:var(--font-mono)] tracking-wide",
                PANEL_BADGE[s.panel] ?? "bg-[var(--surface-3)] text-[var(--text-mute)]"
              )}>
                {PANEL_LABEL[s.panel] ?? s.panel}
              </span>
            </CommandItem>
          ))}
        </CommandGroup>

        <CommandSeparator />

        <CommandGroup heading="Actions">
          <CommandItem value="toggle theme appearance dark light" onSelect={() => { setTheme(theme === "dark" ? "light" : "dark"); setOpen(false); }}>
            <div className={ICON_TILE}>
              <Icon icon={theme === "dark" ? "moon" : "flash"} size={13} className={ICON_CLS} />
            </div>
            <span>Switch to {theme === "dark" ? "Daylight (light)" : "Anduril (dark)"}</span>
          </CommandItem>
          <CommandItem value="alerts notifications security" onSelect={() => fire("toggle-alerts-menu")}>
            <div className={ICON_TILE}>
              <Icon icon="notifications" size={13} className={ICON_CLS} />
            </div>
            <span>Alerts</span>
          </CommandItem>
          <CommandItem value="processes tasks running operations" onSelect={() => fire("toggle-processes-menu")}>
            <div className={ICON_TILE}>
              <Icon icon="processes" size={13} className={ICON_CLS} />
            </div>
            <span>Processes</span>
          </CommandItem>
          <CommandItem value="secure delete shred file folder" onSelect={() => fire("open-shred-dialog")}>
            <div className={ICON_TILE}>
              <Icon icon="trash" size={13} className={ICON_CLS} />
            </div>
            <span>Secure delete…</span>
            <span className="ml-auto shrink-0 rounded border border-[var(--border)] bg-[var(--surface-3)] px-1.5 py-0.5 text-[10px] font-[family-name:var(--font-mono)] text-[var(--text-mute)]">
              shred
            </span>
          </CommandItem>
        </CommandGroup>
      </CommandList>

      <CommandFooter>
        {KBD_HINTS.map(([key, label]) => (
          <span key={key} className="flex items-center gap-1.5 text-[10.5px] text-[var(--text-mute)] font-[family-name:var(--font-mono)]">
            <kbd className="rounded border border-[var(--border)] bg-[var(--surface-3)] px-1.5 py-px text-[10px] font-[family-name:var(--font-mono)]">{key}</kbd>
            {label}
          </span>
        ))}
      </CommandFooter>
    </CommandDialog>
  );
}
