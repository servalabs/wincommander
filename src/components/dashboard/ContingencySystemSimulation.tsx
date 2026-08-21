import { useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { ui } from "@/assets";
import {
  AlertTriangle,
  FileText,
  Folder,
  HardDrive,
  Lock,
  RefreshCcw,
  Server
} from "lucide-react";

type ContingencyPhase = "idle" | "triggered" | "erasing" | "complete" | "restoring";

type SimFolder = {
  id: string;
  name: string;
  sensitive: boolean;
  status: "safe" | "erasing" | "erased";
};

const initialFolders: SimFolder[] = [
  { id: "sensitive", name: "Sensitive", sensitive: true, status: "safe" },
  { id: "movies", name: "Movies", sensitive: false, status: "safe" },
  { id: "games", name: "Games", sensitive: false, status: "safe" }
];

// The two non-sensitive entries exist to show what is left untouched. They must
// not name real films or games: an earlier version used "Avengers_Endgame.mkv"
// and "GTA_VI_Setup.exe", which depicted pirated media as ordinary use of the
// product and put two studios' trademarks in our own UI.
const initialFiles = [
  { id: 1, name: "client_contracts.pdf", sensitive: true, status: "safe" },
  { id: 2, name: "business_data.sql", sensitive: true, status: "safe" },
  { id: 3, name: "team_offsite_video.mkv", sensitive: false, status: "safe" },
  { id: 4, name: "printer_driver_setup.exe", sensitive: false, status: "safe" },
];

const triggers = [
  { id: "panic", title: "Press Physical Button", webm: ui["panic-button.webm"], mp4: ui["panic-button.mp4"] },
  { id: "water", title: "Water the Plant", webm: ui["watering-plant.webm"], mp4: ui["watering-plant.mp4"] },
  { id: "web", title: "Website Click", webm: ui["phone-click.webm"], mp4: ui["phone-click.mp4"] }
];

export default function ContingencySystemSimulation() {
  const [phase, setPhase] = useState<ContingencyPhase>("idle");
  const [step, setStep] = useState(0);
  const [restoreStep, setRestoreStep] = useState(0);
  const [folders, setFolders] = useState<SimFolder[]>(initialFolders);
  const [files, setFiles] = useState(initialFiles);
  const [failedVideos, setFailedVideos] = useState<Record<string, boolean>>({});
  const [readyVideos, setReadyVideos] = useState<Record<string, boolean>>({});

  const handleTrigger = () => {
    if (phase !== "idle") return;
    setPhase("triggered");
    setStep(1);

    setTimeout(() => {
      setStep(2);
      setPhase("erasing");

      // Animate sensitive folder erasing
      setTimeout(() => {
        setFolders((prev) =>
          prev.map((f) => (f.id === "sensitive" ? { ...f, status: "erasing" } : f))
        );
      }, 500);

      setTimeout(() => {
        setFolders((prev) =>
          prev.map((f) => (f.id === "sensitive" ? { ...f, status: "erased" } : f))
        );
        setFiles((prev) =>
          prev.map((f) => (f.sensitive ? { ...f, status: "erased" } : f))
        );
        setStep(3);
      }, 2000);

      setTimeout(() => setStep(4), 3000);
      setTimeout(() => {
        setStep(5);
        setPhase("complete");
      }, 4000);
    }, 1500);
  };

  const handleRestore = () => {
    setPhase("restoring");
    setRestoreStep(0);
    setTimeout(() => setRestoreStep(1), 500);
    setTimeout(() => setRestoreStep(2), 1100);

    // File 1 Restore
    setTimeout(() => {
      setRestoreStep(3);
      setFolders((prev) =>
        prev.map((f) => (f.id === "sensitive" ? { ...f, status: "safe" } : f))
      );
      setFiles((prev) =>
        prev.map((f) => ({ ...f, status: "safe" }))
      );
    }, 1900);

    // File 2 Restore (Animation step)
    setTimeout(() => {
      setRestoreStep(4);
    }, 2900);

    // Complete
    setTimeout(() => {
      setPhase("idle");
      setStep(0);
      setRestoreStep(0);
    }, 4200);
  };

  const mainStatus =
    phase === "restoring"
      ? restoreStep >= 2
        ? "online"
        : "offline"
      : step >= 4
        ? "offline"
        : step >= 2
          ? "erasing"
          : "online";

  const backupStatus =
    phase === "restoring"
      ? restoreStep >= 1
        ? "syncing"
        : "offline"
      : step >= 5
        ? "offline"
        : step >= 4
          ? "erasing"
          : "online";

  return (
    <div className="rounded-xl border border-[var(--color-border)] bg-[var(--color-bg-secondary)] p-5">
      <div className="mb-4 flex items-center justify-between gap-2">
        <div className="flex items-center gap-2">
          <AlertTriangle className="text-rose-500" size={18} />
          <h4 className="text-sm font-bold uppercase tracking-[0.2em] text-[var(--color-text-primary)]">
            Contingency System
          </h4>
        </div>
        <div className="rounded-md border border-rose-500/20 bg-rose-500/10 px-2 py-1 shadow-sm">
          <span className="whitespace-nowrap text-[12px] font-bold uppercase tracking-wide text-rose-600">
            Erase, Then Restore
          </span>
        </div>
      </div>

      <motion.div
        animate={{
          backgroundColor:
            phase === "erasing" || phase === "triggered"
              ? "var(--color-danger)"
              : phase === "complete" || phase === "restoring"
                ? "var(--color-success)"
                : "transparent"
        }}
        className="mb-6 flex items-center justify-between rounded-lg px-4 py-2"
      >
        <span className={`text-[12px] font-medium uppercase tracking-[0.2em] ${phase === "idle" ? "text-[var(--color-text-secondary)]" : "text-white"}`}>
          {phase === "idle" && "System Ready"}
          {phase === "triggered" && "Trigger Detected"}
          {phase === "erasing" && "Erasing Protocol Active"}
          {phase === "complete" && "Sanitization Complete"}
          {phase === "restoring" && "Backup Restore Running"}
        </span>
        {phase === "complete" && (
          <span className="rounded border border-emerald-300/40 bg-emerald-500/20 px-2 py-1 text-[11px] font-bold uppercase text-emerald-200">
            Secure
          </span>
        )}
      </motion.div>

      <div className="space-y-6">
        {phase === "idle" && (
          <div className="grid gap-4 md:grid-cols-3">
            {triggers.map((trigger) => (
              <button
                key={trigger.id}
                onClick={handleTrigger}
                className="group overflow-hidden rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-primary)] p-2.5 text-left transition hover:border-cyan-400/40"
              >
                <div className="relative aspect-[4/3] overflow-hidden rounded border border-[var(--color-border)] bg-[var(--color-bg-tertiary)]">
                  {/* Fallback overlay — shown when no video or video not ready */}
                  {(!readyVideos[trigger.id] || failedVideos[trigger.id]) && (
                    <div className="absolute inset-0 z-[2] flex h-full w-full items-center justify-center bg-[radial-gradient(circle_at_top,rgba(0,242,255,0.18),transparent_55%),linear-gradient(180deg,rgba(8,16,24,0.82),rgba(8,16,24,0.94))] px-4 text-center">
                      <div className="space-y-2">
                        <div className="text-[12px] font-black uppercase tracking-[0.22em] text-cyan-300">
                          {failedVideos[trigger.id] ? "Preview Unavailable" : "Trigger Preview"}
                        </div>
                        <div className="text-xs font-bold text-[var(--color-text-primary)]">
                          {trigger.title}
                        </div>
                      </div>
                    </div>
                  )}
                  {!failedVideos[trigger.id] && (
                    <video
                      className={`absolute inset-0 z-[1] h-full w-full object-cover transition duration-300 group-hover:scale-105 ${readyVideos[trigger.id] ? "opacity-100" : "opacity-0"}`}
                      autoPlay
                      muted
                      loop
                      playsInline
                      preload="auto"
                      onLoadedData={() => setReadyVideos((prev) => ({ ...prev, [trigger.id]: true }))}
                      onCanPlay={() => setReadyVideos((prev) => ({ ...prev, [trigger.id]: true }))}
                      onError={() => setFailedVideos((prev) => ({ ...prev, [trigger.id]: true }))}
                    >
                      <source src={trigger.webm} type="video/webm" />
                      <source src={trigger.mp4} type="video/mp4" />
                    </video>
                  )}
                </div>
                <div className="mt-2.5 text-[12px] font-black uppercase tracking-wide leading-relaxed text-[var(--color-text-primary)]">
                  {trigger.title}
                </div>
                <div className="mt-1 text-[12px] leading-relaxed text-[var(--color-text-muted)]">Click to simulate trigger</div>
              </button>
            ))}
          </div>
        )}

        {phase !== "idle" && (
          <>
            {(phase === "complete" || phase === "restoring") && (
              <div className="flex flex-col items-start justify-between gap-3 rounded-lg border border-emerald-500/30 bg-emerald-500/10 p-3 md:flex-row md:items-center">
                <div className="text-[12px] font-bold uppercase tracking-wider text-emerald-500">
                  Restore from offline backup
                </div>
                <button
                  onClick={handleRestore}
                  className="inline-flex items-center gap-2 rounded bg-emerald-600 px-3 py-2 text-[12px] font-bold uppercase tracking-wider text-white shadow-[0_0_20px_rgba(16,185,129,0.35)]"
                >
                  <RefreshCcw size={12} className={phase === "restoring" ? "animate-spin" : ""} />
                  Restore Systems
                </button>
              </div>
            )}

            <div className="grid grid-cols-2 gap-4">
              <div className="rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-primary)] p-3">
                <div className="mb-1 inline-flex items-center gap-2 text-[12px] font-bold uppercase tracking-wider text-[var(--color-text-secondary)]">
                  <HardDrive size={12} />
                  Main Server
                </div>
                <div className="text-[12px] font-mono font-bold uppercase">
                  {mainStatus === "online" && <span className="text-emerald-600">Online</span>}
                  {mainStatus === "erasing" && <span className="animate-pulse text-amber-600">Erasing</span>}
                  {mainStatus === "offline" && <span className="text-red-600">Offline</span>}
                </div>
              </div>

              <div className="rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-primary)] p-3 relative">
                <div className="mb-1 inline-flex items-center gap-2 text-[12px] font-bold uppercase tracking-wider text-[var(--color-text-secondary)]">
                  <Server size={12} />
                  Backup Server
                </div>
                <div className="text-[12px] font-mono font-bold uppercase">
                  {backupStatus === "online" && <span className="text-emerald-600">Synced</span>}
                  {backupStatus === "syncing" && <span className="animate-pulse text-emerald-500">Restoring</span>}
                  {backupStatus === "offline" && <span className="text-[var(--color-text-muted)]">Offline Safe</span>}
                  {backupStatus === "erasing" && <span className="text-amber-600">Erasing</span>}
                </div>

                {/* File Flying Animation Logic */}
                <AnimatePresence>
                  {phase === "restoring" && restoreStep >= 3 && restoreStep <= 4 && (
                    <motion.div
                      key={`fly-${restoreStep}`}
                      initial={{ x: 0, opacity: 1 }}
                      animate={{ x: -110, opacity: 0 }}
                      exit={{ opacity: 0 }}
                      transition={{ duration: 0.8, ease: "easeInOut" }}
                      className="absolute top-1/2 left-0 -translate-y-1/2 z-50 pointer-events-none"
                    >
                      <div className="bg-black/60 backdrop-blur-sm rounded-md px-2 py-1.5 border border-emerald-500/50 shadow-[0_0_15px_rgba(16,185,129,0.5)]">
                        <FileText className="text-emerald-400 fill-emerald-400/20" size={16} />
                      </div>
                    </motion.div>
                  )}
                </AnimatePresence>
              </div>
            </div>
          </>
        )}

        <div className="mt-8 pt-6">
          <div className="mb-4 flex items-center justify-between">
            <span className="text-[12px] font-bold uppercase tracking-[0.2em] text-[var(--color-text-secondary)]">
              Protected Directories
            </span>
            <span className="text-[12px] font-bold text-[var(--color-text-secondary)]">
              {folders.find((f) => f.id === "sensitive")?.status === "erased" ? "1" : "0"}/1 sensitive cleared
            </span>
          </div>

          <div className="grid grid-cols-3 gap-3">
            <AnimatePresence>
              {folders.map((folder) => (
                <div
                  key={folder.id}
                  className={`relative flex flex-col items-center justify-center gap-2 rounded-xl border-2 py-2 transition-all duration-500 ${folder.status === "erased"
                    ? "border-dashed border-red-500/30 bg-red-500/10"
                    : folder.sensitive
                      ? "border-amber-500/20 bg-amber-500/10"
                      : "border-[var(--color-border)] bg-[var(--color-bg-tertiary)]"
                    }`}
                >
                  {folder.status === "erasing" && (
                    <motion.div
                      className="absolute inset-0 z-10 flex items-center justify-center rounded-xl bg-red-500/20"
                      initial={{ opacity: 0 }}
                      animate={{ opacity: 1 }}
                      exit={{ opacity: 0 }}
                    >
                      <div className="h-full w-full animate-pulse rounded-xl bg-red-600/10" />
                    </motion.div>
                  )}

                  <div className="relative">
                    <AnimatePresence mode="wait">
                      {folder.status === "erased" ? (
                        <motion.div
                          key="erased"
                          initial={{ scale: 0.5, opacity: 0 }}
                          animate={{ scale: 1, opacity: 1 }}
                          className="text-red-300"
                        >
                          <AlertTriangle size={24} />
                        </motion.div>
                      ) : (
                        <motion.div
                          key="folder"
                          animate={
                            folder.status === "erasing"
                              ? {
                                scale: [1, 1.1, 0.9, 1.05, 0.95, 0],
                                opacity: [1, 1, 0.8, 0.5, 0],
                                filter: ["blur(0px)", "blur(2px)", "blur(8px)"]
                              }
                              : { scale: 1, opacity: 1, filter: "blur(0px)" }
                          }
                          transition={{ duration: 1.5 }}
                          className={folder.sensitive ? "text-amber-500" : "text-[var(--color-text-muted)]"}
                        >
                          <Folder size={24} fill="currentColor" fillOpacity={0.2} />
                        </motion.div>
                      )}
                    </AnimatePresence>

                    {folder.sensitive && folder.status !== "erased" && folder.status !== "erasing" && (
                      <div className="absolute -right-1 -top-1 rounded-full bg-amber-500/20 p-0.5 text-amber-600 ring-2 ring-[var(--color-bg-secondary)]">
                        <Lock size={10} />
                      </div>
                    )}
                  </div>

                  <div className="text-center">
                    <div className={`text-[12px] font-bold uppercase tracking-wide ${folder.status === "erased" ? "text-red-400 line-through" : "text-[var(--color-text-secondary)]"
                      }`}>
                      {folder.name}
                    </div>
                    <div className="text-[11px] font-medium text-[var(--color-text-muted)]">
                      {folder.status === "erased" ? "Erased" : folder.sensitive ? "Encrypted" : "Standard"}
                    </div>
                  </div>
                </div>
              ))}
            </AnimatePresence>
          </div>

          <div className="mt-6 pt-4">
            <div className="mb-3 text-[12px] font-bold uppercase tracking-[0.2em] text-[var(--color-text-muted)]">
              File System View
            </div>
            <div className="space-y-2">
              {files.map((file) => (
                <div
                  key={file.id}
                  className={`flex items-center justify-between rounded-lg border px-3 py-2 text-[12px] ${file.status === "erased"
                    ? "border-red-500/20 bg-red-500/10 text-red-600"
                    : "border-[var(--color-border)] bg-[var(--color-bg-primary)] text-[var(--color-text-secondary)]"
                    }`}
                >
                  <div className="flex items-center gap-2">
                    <FileText size={12} className={file.status === "erased" ? "text-red-400" : "text-[var(--color-text-muted)]"} />
                    <span className={file.status === "erased" ? "line-through opacity-50" : ""}>
                      {file.name}
                    </span>
                  </div>
                  <span className="text-[11px] font-bold uppercase">
                    {file.status === "erased" ? "Deleted" : "Secure"}
                  </span>
                </div>
              ))}
            </div>
          </div>

          <div className="mt-4 rounded-lg bg-[var(--color-bg-tertiary)] p-3">
            <p className="text-[12px] uppercase leading-relaxed text-[var(--color-text-muted)] font-medium">
              <strong className="text-[var(--color-text-secondary)]">Demo:</strong> This is a simulation of an <span className="text-[var(--color-text-primary)] font-bold">Emergency Lockdown</span>.
              In a crisis, triggering the system physically or remotely <span className="text-red-600 font-bold">erases sensitive data</span> from the active server while keeping a <span className="text-emerald-600 font-bold">secure offline backup</span> you can restore from.
              For your own data only &mdash; never to destroy evidence or block a lawful order.
            </p>
          </div>
        </div>
      </div>
    </div>
  );
}
