import { useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import {
  Camera,
  Folder,
  Globe,
  Lock,
  LockOpen,
  Mail,
  MessageSquare,
  Phone,
  Settings,
  Trash2
} from "lucide-react";

type PhoneState = "locked" | "unlocking" | "unlocked" | "erasing" | "erased";

const NORMAL_PIN = "1234";
const DURESS_PIN = "9999";

const apps = [
  { name: "Camera", Icon: Camera, color: "from-rose-600 to-red-700" },
  { name: "Signal", Icon: MessageSquare, color: "from-amber-600 to-orange-700" },
  { name: "Files", Icon: Folder, color: "from-cyan-600 to-blue-700" },
  { name: "Settings", Icon: Settings, color: "from-slate-600 to-slate-700" },
  { name: "Vault", Icon: Lock, color: "from-emerald-600 to-teal-700" },
  { name: "Phone", Icon: Phone, color: "from-green-500 to-emerald-600" },
  { name: "Tor", Icon: Globe, color: "from-purple-600 to-violet-700" },
  { name: "Mail", Icon: Mail, color: "from-blue-600 to-cyan-700" }
];

export default function PrivatePhoneSimulation() {
  const [pin, setPin] = useState("");
  const [status, setStatus] = useState<PhoneState>("locked");
  const [progress, setProgress] = useState(0);
  const [shake, setShake] = useState(false);
  const [activeKey, setActiveKey] = useState<string | null>(null);
  const idle = status === "locked";

  const resetToLocked = () => {
    setStatus("locked");
    setPin("");
    setProgress(0);
  };

  const checkPin = (enteredPin: string) => {
    if (enteredPin === NORMAL_PIN) {
      setStatus("unlocking");
      setTimeout(() => {
        setStatus("unlocked");
        setTimeout(resetToLocked, 3000);
      }, 550);
      return;
    }

    if (enteredPin === DURESS_PIN) {
      setStatus("erasing");
      let current = 0;
      const timer = setInterval(() => {
        current += 4;
        setProgress(current);
        if (current >= 100) {
          clearInterval(timer);
          setStatus("erased");
          setTimeout(resetToLocked, 3000);
        }
      }, 100);
      return;
    }

    setShake(true);
    setTimeout(() => {
      setPin("");
      setShake(false);
    }, 450);
  };

  const handleKeyPress = (key: string) => {
    if (status !== "locked" || pin.length >= 4) return;
    const next = pin + key;
    setPin(next);
    if (next.length === 4) checkPin(next);
  };

  const handleClear = () => {
    if (status !== "locked") return;
    setPin((prev) => prev.slice(0, -1));
  };

  const autoType = (targetPin: string) => {
    if (status !== "locked") return;
    setPin("");
    targetPin.split("").forEach((digit, i) => {
      setTimeout(() => {
        setActiveKey(digit);
        setTimeout(() => {
          setPin((prev) => prev + digit);
          setActiveKey(null);
        }, 100);
      }, i * 250);
    });
    setTimeout(() => checkPin(targetPin), targetPin.length * 250 + 180);
  };

  return (
    <div className="relative flex h-full min-h-[380px] flex-col rounded-xl border border-[var(--color-border)] bg-[var(--color-bg-secondary)] p-4 md:p-5">
      {/* Top-right corner feature tags — sit above the phone area in the
          card's header band so they never overlap the phone screen itself. */}
      <div className="pointer-events-none absolute right-3 top-3 z-10 flex flex-col items-end gap-1">
        {["No Google", "No Tracking", "On-Device"].map((tag) => (
          <div
            key={tag}
            className="flex items-center gap-1.5 whitespace-nowrap rounded-full border border-emerald-500/30 bg-emerald-500/10 px-2 py-0.5 text-[10px] font-bold uppercase tracking-[0.14em] text-emerald-700 backdrop-blur-sm dark:border-emerald-400/30 dark:bg-emerald-400/10 dark:text-emerald-300"
          >
            <span className="inline-block h-1.5 w-1.5 rounded-full bg-emerald-500 dark:bg-emerald-400" />
            {tag}
          </div>
        ))}
      </div>

      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="flex items-center gap-2">
            <Phone className="text-cyan-700" size={18} />
            <h4 className="whitespace-nowrap text-sm font-bold uppercase tracking-[0.2em] text-[var(--color-text-primary)]">
              Private Phone
            </h4>
          </div>
          <div className="ml-7 mt-0.5 text-[11px] font-bold uppercase tracking-wider text-[var(--color-text-muted)]">
            Panic PIN erases second profile.
          </div>
        </div>
      </div>

      <div className="mt-3 flex flex-1 flex-col items-center justify-center">
        <div className="flex w-full flex-1 items-center justify-center rounded-[1.75rem] border border-cyan-500/12 bg-[radial-gradient(circle_at_top,rgba(0,242,255,0.08),transparent_58%),var(--color-bg-primary)] px-4 py-4">
          <div className="relative mx-auto flex w-full max-w-[278px] items-center justify-center overflow-visible">
            <div className="w-full max-w-[212px] shrink-0 rounded-[2.05rem] border border-slate-400 bg-gradient-to-b from-slate-200 to-slate-300 p-[7px] shadow-2xl">
              <div className="relative aspect-[9/16.9] w-full overflow-hidden rounded-[1.72rem] border-[3px] border-black bg-white">
                <div className="absolute left-1/2 top-2 z-20 h-1.5 w-16 -translate-x-1/2 rounded-full bg-black/85" />
                <AnimatePresence mode="wait">
                  {status === "locked" && (
                    <motion.div
                      key="locked"
                      initial={{ opacity: 0 }}
                      animate={{ opacity: 1 }}
                      exit={{ opacity: 0 }}
                      className="absolute inset-0 flex flex-col px-3 pb-2 pt-5"
                    >
                      <motion.div
                        animate={shake ? { x: [0, -8, 8, -8, 8, 0] } : {}}
                        transition={{ duration: 0.35 }}
                        className="flex flex-1 flex-col justify-between gap-2"
                      >
                        <div className="pt-4">
                          <div className="mb-2 text-center">
                            <motion.div
                              animate={{ y: [0, -3, 0], opacity: [0.78, 1, 0.78] }}
                              transition={{ duration: 1.55, repeat: Infinity, ease: "easeInOut" }}
                              className="text-[11px] font-black uppercase tracking-[0.2em] text-cyan-700"
                            >
                              Try the demo
                            </motion.div>
                          </div>
                          <div className="grid grid-cols-2 gap-2">
                            <button
                              className="rounded-xl border border-cyan-300/40 bg-gradient-to-br from-cyan-500 to-blue-600 px-2 py-2 text-center text-white shadow-[0_0_18px_rgba(6,182,212,0.28)] transition active:scale-95"
                              onClick={() => autoType(NORMAL_PIN)}
                              disabled={!idle}
                            >
                              <div className="font-mono text-sm font-black leading-none">1234</div>
                              <div className="mt-1 text-[9px] font-black uppercase tracking-[0.12em]">Unlock</div>
                            </button>
                            <button
                              className="rounded-xl border border-red-300/50 bg-red-50 px-2 py-2 text-center text-red-600 transition active:scale-95"
                              onClick={() => autoType(DURESS_PIN)}
                              disabled={!idle}
                            >
                              <div className="font-mono text-sm font-black leading-none">9999</div>
                              <div className="mt-1 text-[9px] font-black uppercase tracking-[0.12em]">Lock</div>
                            </button>
                          </div>
                        </div>

                        <div className="pb-3">
                          <div className="mb-8 flex -translate-y-5 justify-center gap-2">
                            {[0, 1, 2, 3].map((i) => (
                              <div
                                key={i}
                                className={`h-2.5 w-2.5 rounded-full ${i < pin.length ? "bg-slate-900" : "bg-slate-200"}`}
                              />
                            ))}
                          </div>

                          <div className="grid grid-cols-3 gap-2 px-1">
                            {["1", "2", "3", "4", "5", "6", "7", "8", "9", "", "0", "⌫"].map((key) => (
                              <button
                                key={key || "empty"}
                                disabled={!key}
                                onClick={() => (key === "⌫" ? handleClear() : handleKeyPress(key))}
                                className={`h-11 w-11 rounded-full text-base font-semibold transition-all ${!key
                                  ? "opacity-0"
                                  : activeKey === key
                                    ? "scale-90 border border-cyan-400 bg-cyan-500 text-white"
                                    : "border border-slate-200 bg-white text-slate-900 active:bg-slate-50"
                                  }`}
                              >
                                {key}
                              </button>
                            ))}
                          </div>
                        </div>
                      </motion.div>
                    </motion.div>
                  )}

                  {status === "unlocking" && (
                    <motion.div
                      key="unlocking"
                      initial={{ opacity: 0 }}
                      animate={{ opacity: 1 }}
                      exit={{ opacity: 0 }}
                      className="absolute inset-0 flex items-center justify-center bg-white/70 backdrop-blur-sm"
                    >
                      <motion.div
                        initial={{ scale: 0.8, opacity: 0 }}
                        animate={{ scale: 1, opacity: 1 }}
                        className="rounded-full bg-cyan-500 p-5 text-white"
                      >
                        <LockOpen />
                      </motion.div>
                    </motion.div>
                  )}

                  {status === "unlocked" && (
                    <motion.div
                      key="unlocked"
                      initial={{ opacity: 0 }}
                      animate={{ opacity: 1 }}
                      exit={{ opacity: 0 }}
                      className="absolute inset-0 bg-gradient-to-b from-slate-100 to-white px-4 pb-4 pt-7"
                    >
                      <div className="mb-4 flex items-center justify-between">
                        <div className="text-[10px] font-black uppercase tracking-[0.18em] text-cyan-700">
                          Private Mode
                        </div>
                        <div className="rounded-full bg-emerald-500/12 px-2 py-1 text-[9px] font-black uppercase tracking-[0.16em] text-emerald-700 dark:text-emerald-300">
                          Hardened
                        </div>
                      </div>
                      <div className="grid grid-cols-4 gap-2">
                        {apps.map((app) => (
                          <div key={app.name} className="flex flex-col items-center gap-1">
                            <div className={`flex h-8 w-8 items-center justify-center rounded-lg bg-gradient-to-br ${app.color} text-white`}>
                              <app.Icon size={13} />
                            </div>
                            <span className="text-[9px] font-semibold text-slate-700">{app.name}</span>
                          </div>
                        ))}
                      </div>
                    </motion.div>
                  )}

                  {status === "erasing" && (
                    <motion.div
                      key="erasing"
                      initial={{ opacity: 0 }}
                      animate={{ opacity: 1 }}
                      exit={{ opacity: 0 }}
                      className="absolute inset-0 flex flex-col items-center justify-center bg-gradient-to-b from-red-950 to-slate-950 px-6"
                    >
                      <div className="mb-4 text-sm font-black uppercase tracking-[0.2em] text-red-600">
                        Factory Reset
                      </div>
                      <div className="h-2 w-full overflow-hidden rounded-full border border-red-400/40 bg-red-950/40">
                        <motion.div className="h-full bg-red-600" style={{ width: `${progress}%` }} />
                      </div>
                      <div className="mt-2 text-xs font-mono text-red-600">{progress}%</div>
                    </motion.div>
                  )}

                  {status === "erased" && (
                    <motion.div
                      key="erased"
                      initial={{ opacity: 0 }}
                      animate={{ opacity: 1 }}
                      className="absolute inset-0 flex flex-col items-center justify-center gap-3 bg-slate-950"
                    >
                      <Trash2 className="text-red-600" />
                      <div className="text-xs font-black uppercase tracking-[0.2em] text-red-600">
                        Device Erased
                      </div>
                      <button
                        className="rounded-lg border border-slate-600 bg-slate-800 px-3 py-2 text-[12px] font-bold uppercase text-slate-100"
                        onClick={resetToLocked}
                      >
                        Restart Demo
                      </button>
                    </motion.div>
                  )}
                </AnimatePresence>

                {status === "unlocking" && (
                  <div className="pointer-events-none absolute right-4 top-6 text-cyan-600">
                    <Lock size={14} />
                  </div>
                )}
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
