import { useRef, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { products } from "@/assets";
import {
  ArrowRight,
  CheckCircle2,
  FileText,
  Folder,
  HardDrive,
  Lock,
  Shield,
  Unlock,
  XCircle,
} from "lucide-react";

// Standard decoy items (E:) -> totals 24 GB
const decoyItems = [
  { icon: Folder, label: "Movies", size: "8 GB" },
  { icon: Folder, label: "Music", size: "3 GB" },
  { icon: Folder, label: "Photos", size: "6 GB" },
  { icon: Folder, label: "Games", size: "6 GB" },
  { icon: FileText, label: "Homework", size: "720 MB" },
  { icon: FileText, label: "Notes", size: "280 MB" },
];

// Secure hidden items (X:) -> totals 30 GB
const realItems = [
  { name: "Bank_Accounts.xlsx", size: "2.4 MB", type: "file" },
  { name: "Crypto_Keys.txt", size: "4 KB", type: "file" },
  { name: "Passport.pdf", size: "14 MB", type: "file" },
  { name: "Business_Contracts", size: "156 MB", type: "folder" },
  { name: "Tax_Returns_2025.pdf", size: "1.8 GB", type: "file" },
  { name: "Customer_DB.sqlite", size: "10.0 GB", type: "file" },
  { name: "Secure_Backup_Archive", size: "18.0 GB", type: "folder" },
];

type HubState = "locked" | "decoy" | "real" | "wrong";

const STATUS_THEMES: Record<HubState, { label: string; icon: typeof Lock }> = {
  locked: { label: "Waiting for credentials", icon: Lock },
  decoy: { label: "Standard volume mounted", icon: Folder },
  real: { label: "Hidden volume mounted", icon: CheckCircle2 },
  wrong: { label: "Invalid password credentials", icon: XCircle },
};

export default function DecoyHub() {
  const [password, setPassword] = useState("");
  const [status, setStatus] = useState<HubState>("locked");
  const [typing, setTyping] = useState<"fakepass" | "realpass" | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  const resolvePassword = (value: string) => {
    setTyping(null);
    if (value === "fakepass") {
      setStatus("decoy");
    } else if (value === "realpass") {
      setStatus("real");
    } else {
      setStatus("wrong");
    }
  };

  const handlePasswordChange = (val: string) => {
    if (status !== "locked") {
      setStatus("locked");
      setTyping(null);
    }
    setPassword(val);
  };

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!password.trim()) return;
    resolvePassword(password);
  };

  const autoType = (value: "fakepass" | "realpass") => {
    setPassword(value);
    resolvePassword(value);
  };

  const realActive = status === "real";
  const decoyActive = status === "decoy";
  const hasResolvedState = decoyActive || realActive;
  const canUnlock = password.trim().length > 0;
  const theme = STATUS_THEMES[status];
  const StatusIcon = theme.icon;

  return (
    <div className="relative flex h-full min-h-[380px] flex-col rounded-xl border border-[var(--color-border)] bg-[var(--color-bg-secondary)] p-4 md:p-5">
      <style>{`
        .naked-hub-img {
          border-radius: 12px !important;
        }
        .naked-hub-img-container {
          border-radius: 12px !important;
        }
        .active-tag-overlay {
          border-radius: 6px !important;
        }
        .password-input-wrapper {
          margin-bottom: 16px !important;
        }
      `}</style>
      {/* ── HEADER ─────────────────────────────────────────────────── */}
      <div className="mb-4 flex items-center justify-between gap-3 pb-0">
        <div className="min-w-0">
          <div className="flex items-center gap-2">
            <HardDrive className="text-[var(--color-accent)]" size={18} />
            <h4 className="text-sm font-black uppercase tracking-[0.18em] text-[var(--color-text-primary)]">
              Decoy Hub
            </h4>
          </div>
          <p className="ml-7 mt-0.5 text-[12px] font-bold text-[var(--color-text-secondary)]">
            One physical device · two passwords · absolute deniability
          </p>
        </div>

      </div>

      {/* ── STATUS BAR ──────────────────────────────────────────────── */}
      {status !== "locked" && (
        <motion.div
          animate={{
            backgroundColor:
              status === "decoy"
                ? "var(--color-warning-dim)"
                : status === "real"
                  ? "var(--color-accent-dim)"
                  : status === "wrong"
                    ? "var(--color-danger-dim)"
                    : "var(--color-bg-tertiary)"
          }}
          className="mb-4 flex items-center justify-between rounded-lg border border-[var(--color-border)] px-4 py-2 transition-all"
        >
          <div className="flex items-center gap-2">
            <StatusIcon className={`h-4 w-4 ${status === "decoy" ? "text-[var(--color-warning)]" : status === "wrong" ? "text-[var(--color-danger)]" : "text-[var(--color-accent)]"}`} />
            <div className="flex items-center gap-2">
              <span className={`inline-block h-2 w-2 rounded-full ${status === "decoy" ? "bg-[var(--color-warning)]" : status === "wrong" ? "bg-[var(--color-danger)]" : "bg-[var(--color-accent)]"}`} />
              <span className={`text-[12px] font-black uppercase tracking-[0.18em] ${status === "decoy" ? "text-[var(--color-warning)]" : status === "wrong" ? "text-[var(--color-danger)]" : "text-[var(--color-text-primary)]"}`}>
                {theme.label}
              </span>
            </div>
          </div>
        </motion.div>
      )}

      {/* ── CORE STACK ─────────────────────────────────────────────── */}
      <div className="flex flex-1 flex-col gap-4">
        {/* ROW 1: Side-by-side Studio Image & Inputs */}
        <div className="flex flex-col gap-4 sm:flex-row">
          {/* 
            KNOWLEDGE TRANSFER (KT) NOTE:
            Chromium/WebKit (Tauri's runtime webview) has a known GPU rendering bug where hardware-accelerated 
            children (like Framer Motion's `<motion.img>`) bypass parent `overflow: hidden` border-radius clipping.
            
            TO PREVENT CORNER BLEEDING & TROUBLESHOOTING RECURRENCE:
            1. Use `object-cover` so the image background stretches to touch all four edges.
            2. Force GPU layer isolation with `isolation: "isolate"`.
            3. Apply `-webkit-radial-gradient(white, black)` as a WebkitMaskImage to force clipping. 
            Do NOT strip these properties or the image corners will revert to sharp edges inside the card.
          */}
          <div className="relative flex w-full items-center justify-center overflow-hidden sm:w-[250px] shrink-0 border border-[var(--color-border)] bg-[var(--color-bg-tertiary)] naked-hub-img-container" style={{ height: "200px", borderRadius: "12px", marginTop: "12px", isolation: "isolate", WebkitMaskImage: "-webkit-radial-gradient(white, black)" }}>
            <motion.img
              initial={{ opacity: 0, scale: 0.92 }}
              animate={{ opacity: 1, scale: 1 }}
              transition={{ duration: 0.45 }}
              src={products["contingency/decoy-hub.png"]}
              alt="Storage Hub Device"
              className="relative z-10 h-full w-full object-cover naked-hub-img"
              style={{ borderRadius: "12px" }}
            />

            {/* Active-password overlay tag */}
            <AnimatePresence>
              {(typing || hasResolvedState) && (
                <motion.div
                  initial={{ opacity: 0, y: 6 }}
                  animate={{ opacity: 1, y: 0 }}
                  exit={{ opacity: 0, y: 6 }}
                  className="absolute bottom-2 left-2 z-20 flex items-center gap-1.5 border border-[var(--color-border-accent)] bg-[var(--color-bg-secondary)] px-2 py-1 shadow-md backdrop-blur-sm active-tag-overlay"
                  style={{ borderRadius: "6px" }}
                >
                  <span className="font-mono text-[11px] font-black tracking-wider text-[var(--color-accent)]">
                    {typing === "realpass" || realActive ? "realpass" : "fakepass"}
                  </span>
                  <ArrowRight className="h-2.5 w-2.5 text-[var(--color-accent)]" />
                  <span className="text-[10px] font-black uppercase tracking-wider text-[var(--color-text-secondary)]">
                    {realActive ? "Drive X:" : decoyActive ? "Drive E:" : "…"}
                  </span>
                </motion.div>
              )}
            </AnimatePresence>
          </div>

          {/* Input Controls */}
          <div className="flex min-w-0 flex-1 flex-col justify-start gap-2.5">
            <div style={{ marginTop: "12px" }}>
              <div className="text-[12px] font-black uppercase tracking-wider text-[var(--color-text-primary)]">
                Enter a password to unlock
              </div>
              <p className="mt-0.5 text-[12px] text-[var(--color-text-muted)]">
                Or tap one of the demo passwords below.
              </p>
            </div>

            <form onSubmit={handleSubmit} className="flex flex-col" style={{ display: "flex", flexDirection: "column" }}>
              <div className="password-input-wrapper">
                <motion.div
                  animate={
                    status === "locked"
                      ? {
                        boxShadow: [
                          "0 0 0 rgba(0, 242, 255, 0)",
                          "0 0 12px rgba(0, 242, 255, 0.15)",
                          "0 0 0 rgba(0, 242, 255, 0)",
                        ],
                      }
                      : { boxShadow: "0 0 0 rgba(0, 242, 255, 0)" }
                  }
                  transition={status === "locked" ? { duration: 1.8, repeat: Infinity, ease: "easeInOut" } : { duration: 0.2 }}
                  className={`flex items-center gap-2.5 rounded-lg border px-3 py-2 transition-all ${status === "wrong"
                    ? "border-[var(--color-danger)] bg-[var(--color-danger-dim)]"
                    : status === "real"
                      ? "border-[var(--color-border-accent)] bg-[var(--color-accent-dim)]"
                      : status === "decoy"
                        ? "border-[var(--color-warning)] bg-[var(--color-warning-dim)]"
                        : "border-[var(--color-border)] bg-[var(--color-bg-primary)] focus-within:border-[var(--color-accent)]"
                    }`}
                >
                  <div className="flex h-7 w-7 shrink-0 items-center justify-center rounded bg-[var(--color-bg-tertiary)] text-[var(--color-accent)]">
                    {status === "real" ? <Unlock className="h-3.5 w-3.5" /> : <Lock className="h-3.5 w-3.5" />}
                  </div>
                  <input
                    ref={inputRef}
                    type="password"
                    value={password}
                    onChange={(e) => handlePasswordChange(e.target.value)}
                    placeholder="Type a password…"
                    className="w-full bg-transparent font-mono text-[12px] tracking-[0.22em] text-[var(--color-text-primary)] placeholder-[var(--color-text-muted)] focus:outline-none"
                  />
                </motion.div>
              </div>

              <div style={{ height: "16px" }} />

              <button
                disabled={!canUnlock}
                className={`w-full rounded-lg px-4 py-2 text-[12px] font-black uppercase tracking-wider transition ${!canUnlock
                  ? "bg-[var(--color-bg-tertiary)] border border-[var(--color-border)] text-[var(--color-text-muted)] cursor-not-allowed"
                  : "bg-[var(--color-accent-subtle)] border border-[var(--color-accent)] text-[var(--color-accent)] hover:bg-[var(--color-accent-dim)] hover:shadow-[0_0_10px_var(--color-accent-dim)] cursor-pointer"
                  }`}
              >
                {status === "wrong"
                  ? "Access denied - try again"
                  : "Unlock"}
              </button>
            </form>

            <div className="grid gap-2 grid-cols-2">
              <button
                onClick={() => autoType("fakepass")}
                className="group flex items-center justify-between gap-2 rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-tertiary)] px-2.5 py-1.5 text-left transition hover:border-[var(--color-warning)] hover:bg-[var(--color-warning-dim)]/20"
              >
                <div className="min-w-0">
                  <div className="font-mono text-[12px] font-black tracking-wider text-[var(--color-warning)]">
                    fakepass
                  </div>
                  <div className="mt-0.5 text-[10px] uppercase tracking-wider text-[var(--color-text-muted)] leading-none">
                    Decoy E:
                  </div>
                </div>
                <ArrowRight className="h-3 w-3 shrink-0 text-[var(--color-warning)] transition-transform group-hover:translate-x-0.5" />
              </button>

              <button
                onClick={() => autoType("realpass")}
                className="group flex items-center justify-between gap-2 rounded-lg border border-[var(--color-accent)] bg-[var(--color-accent-dim)] px-2.5 py-1.5 text-left transition hover:border-[var(--color-accent-hover)] hover:bg-[var(--color-accent-dim)]/50"
              >
                <div className="min-w-0">
                  <div className="font-mono text-[12px] font-black tracking-wider text-[var(--color-accent)]">
                    realpass
                  </div>
                  <div className="mt-0.5 text-[10px] uppercase tracking-wider text-[var(--color-text-secondary)] leading-none">
                    Hidden X:
                  </div>
                </div>
                <ArrowRight className="h-3 w-3 shrink-0 text-[var(--color-accent)] transition-transform group-hover:translate-x-0.5" />
              </button>
            </div>
          </div>
        </div>

        {/* ROW 2: Mounted Files / Idle explanation */}
        <div className="flex min-h-[160px] flex-col flex-1">
          <AnimatePresence mode="wait">
            {realActive ? (
              <motion.div
                key="real"
                initial={{ opacity: 0, y: 10 }}
                animate={{ opacity: 1, y: 0 }}
                exit={{ opacity: 0, y: -10 }}
                className="flex h-full flex-col overflow-hidden rounded-xl border border-[var(--color-border-accent)] bg-[var(--color-accent-dim)]/30"
              >
                <div className="flex items-center justify-between gap-2 border-b border-[var(--color-border-accent)] px-3.5 py-2">
                  <div className="flex items-center gap-2">
                    <Shield className="h-4 w-4 text-[var(--color-accent)]" />
                    <h5 className="text-[12px] font-black uppercase tracking-wider text-[var(--color-accent)]">
                      Drive X: · {realItems.length} items
                    </h5>
                  </div>
                  <span className="text-[11px] font-bold tracking-wide text-[var(--color-accent)]/80">
                    30 GB · NTFS · Hidden Container
                  </span>
                </div>
                <div className="flex-1 overflow-y-auto p-2">
                  <div className="space-y-1.5">
                    {realItems.map((file, i) => (
                      <motion.div
                        key={file.name}
                        initial={{ opacity: 0, x: -8 }}
                        animate={{ opacity: 1, x: 0 }}
                        transition={{ delay: i * 0.07 }}
                        className="flex items-center justify-between gap-3 rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-primary)] px-3 py-1.5"
                      >
                        <div className="flex min-w-0 items-center gap-2">
                          {file.type === "folder" ? (
                            <Folder className="h-4 w-4 shrink-0 text-[var(--color-accent)]" />
                          ) : (
                            <FileText className="h-4 w-4 shrink-0 text-[var(--color-accent)]" />
                          )}
                          <span className="truncate font-mono text-[12px] text-[var(--color-text-primary)]">{file.name}</span>
                        </div>
                        <span className="shrink-0 font-mono text-[12px] text-[var(--color-text-muted)]">{file.size}</span>
                      </motion.div>
                    ))}
                  </div>
                </div>
              </motion.div>
            ) : decoyActive ? (
              <motion.div
                key="decoy"
                initial={{ opacity: 0, y: 10 }}
                animate={{ opacity: 1, y: 0 }}
                exit={{ opacity: 0, y: -10 }}
                className="flex h-full flex-col overflow-hidden rounded-xl border border-[var(--color-warning)] bg-[var(--color-warning-dim)]/30"
              >
                <div className="flex items-center justify-between gap-2 border-b border-[var(--color-warning)] px-3.5 py-2">
                  <div className="flex items-center gap-2">
                    <Folder className="h-4 w-4 text-[var(--color-warning)]" />
                    <h5 className="text-[12px] font-black uppercase tracking-wider text-[var(--color-warning)]">
                      Drive E: · {decoyItems.length} items
                    </h5>
                  </div>
                  <span className="text-[11px] font-bold tracking-wide text-[var(--color-warning)]/80">
                    24 GB · FAT32 · Standard Decoy
                  </span>
                </div>
                <div className="flex-1 overflow-y-auto p-2">
                  <div className="space-y-1.5">
                    {decoyItems.map((item, i) => (
                      <motion.div
                        key={item.label}
                        initial={{ opacity: 0, x: -8 }}
                        animate={{ opacity: 1, x: 0 }}
                        transition={{ delay: i * 0.07 }}
                        className="flex items-center justify-between gap-3 rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-primary)] px-3 py-1.5"
                      >
                        <div className="flex min-w-0 items-center gap-2">
                          <item.icon className="h-4 w-4 shrink-0 text-[var(--color-warning)]" />
                          <span className="truncate font-mono text-[12px] text-[var(--color-text-primary)]">{item.label}</span>
                        </div>
                        <span className="shrink-0 font-mono text-[12px] text-[var(--color-text-muted)]">{item.size}</span>
                      </motion.div>
                    ))}
                  </div>
                </div>
              </motion.div>
            ) : (
              <motion.div
                key="idle"
                initial={{ opacity: 0 }}
                animate={{ opacity: 1 }}
                exit={{ opacity: 0 }}
                className="flex h-full flex-col items-center justify-center rounded-xl border border-dashed border-[var(--color-border)] bg-[var(--color-bg-tertiary)] px-4 py-5 text-center"
              >
                <Lock className="mb-2 h-5 w-5 text-[var(--color-text-muted)]" />
                <p className="text-[12px] font-bold uppercase tracking-wider text-[var(--color-text-secondary)]">
                  Choose a password to mount a partition
                </p>
                <p className="mt-1 text-[12px] leading-relaxed text-[var(--color-text-muted)] max-w-md mx-auto">
                  Entering standard credentials opens the unencrypted <span className="text-[var(--color-warning)] font-bold">Decoy Volume E:</span>.
                  Entering secure credentials mounts the encrypted <span className="text-[var(--color-accent)] font-bold">Hidden Volume X:</span> that visually occupies the empty free space of the drive.
                </p>
              </motion.div>
            )}
          </AnimatePresence>
        </div>

        {/* ROW 3: Plausible Deniability Partition Mapping / Storage Bar */}
        <div className="flex flex-col gap-2 rounded-xl border border-[var(--color-border)] bg-[var(--color-bg-tertiary)] p-3">
          <div className="flex items-center justify-between text-[11px] font-black uppercase tracking-wider">
            <span className="text-[var(--color-text-secondary)]">Plausible Deniability Partition Mapping</span>
            <span className="text-[var(--color-text-muted)] font-mono normal-case tracking-normal">
              Physical Disk Capacity: 64.0 GB
            </span>
          </div>

          {/* Block-allocation bar based on VeraCrypt inner partition structures */}
          <div className="relative flex h-10 w-full overflow-hidden rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-primary)]">
            {/* 1. Header of the Standard Volume (Green) — Always visible */}
            <div
              className="h-full bg-emerald-600 transition-all duration-300"
              style={{ width: "4%" }}
              title="Header of the Standard Volume"
            />

            {/* 2. Header of the Hidden Volume (Teal) — Right after the standard header, animates in on realActive */}
            <motion.div
              initial={{ width: "0%" }}
              animate={{ width: realActive ? "4%" : "0%" }}
              transition={{ duration: 0.45 }}
              className="h-full bg-cyan-500 overflow-hidden"
              title="Header of the Hidden Volume"
            />

            {/* 3. Space Occupied by Decoy Files (Purple) — Completely hidden in real/hidden active state! */}
            {!realActive && (
              <div
                className="h-full bg-violet-600 transition-all duration-300 flex items-center justify-center min-w-[20px]"
                style={{ width: "36%" }}
                title="Standard Volume E: Space Occupied by Files (24.0 GB)"
              >
                <span className="text-[10px] font-black text-white uppercase tracking-wider hidden sm:inline">E:\ 24G</span>
              </div>
            )}

            {/* 4. Separator */}
            {!realActive && <div className="h-full w-[1px] bg-black/20" />}

            {/* 5. Free Space (Grey/Tertiary) — Fills the remaining space before the Hidden volume at the end */}
            <div
              className="h-full bg-slate-500/20 flex-1 flex items-center justify-center border-r border-black/20"
              title="Free Space (Random Data / Padding)"
            >
              <span className="text-[10px] font-medium text-[var(--color-text-muted)] uppercase tracking-wider hidden sm:inline">
                {realActive ? "Free 30G" : "Free 40G"}
              </span>
            </div>

            {/* 6. Hidden Volume Data Area (Dark Blue) — Carved out of standard volume's random-data free space at the very end! */}
            <motion.div
              animate={{
                backgroundColor: realActive ? "#1d4ed8" : "var(--color-bg-tertiary)",
                width: realActive ? "50%" : "0%",
              }}
              transition={{ duration: 0.55 }}
              className="h-full flex items-center justify-center overflow-hidden"
              title="Hidden Volume X: (Secure Data Area: 30.0 GB)"
            >
              {realActive && (
                <span className="text-[10px] font-black text-white uppercase tracking-wider hidden sm:inline">X:\ 30G (Hidden)</span>
              )}
            </motion.div>
          </div>

          {/* Color legends matching the block diagram exactly */}
          <div className="flex flex-wrap items-center gap-x-4 gap-y-1.5 pt-1.5 border-t border-[var(--color-border)]/50">
            <div className="flex items-center gap-1.5 text-[11px] font-bold uppercase tracking-wider text-[var(--color-text-secondary)]">
              <span className="h-2 w-2 rounded bg-emerald-600" />
              <span>Standard Header</span>
            </div>
            {realActive && (
              <div className="flex items-center gap-1.5 text-[11px] font-bold uppercase tracking-wider text-[var(--color-text-secondary)]">
                <span className="h-2 w-2 rounded bg-cyan-500" />
                <span>Hidden Header</span>
              </div>
            )}
            {!realActive && (
              <div className="flex items-center gap-1.5 text-[11px] font-bold uppercase tracking-wider text-[var(--color-text-secondary)]">
                <span className="h-2 w-2 rounded bg-violet-600" />
                <span>E: Decoy Files (24 GB)</span>
              </div>
            )}
            {realActive ? (
              <div className="flex items-center gap-1.5 text-[11px] font-bold uppercase tracking-wider text-cyan-400 animate-pulse">
                <span className="h-2 w-2 rounded bg-blue-700" />
                <span>X: Hidden Volume (30 GB)</span>
              </div>
            ) : (
              <div className="flex items-center gap-1.5 text-[11px] font-bold uppercase tracking-wider text-[var(--color-text-muted)]">
                <span className="h-2 w-2 rounded bg-slate-500/20" />
                <span>Free Space (40 GB)</span>
              </div>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
