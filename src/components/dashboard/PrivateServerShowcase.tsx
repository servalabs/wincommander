import { useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { Cpu, HardDrive, Server, Shield } from "lucide-react";
import AnimatedNumber from "@/components/shared/AnimatedNumber";
import { DURATION_S } from "@/components/shared/motion";
import { products, saas } from "@/assets";
import "./PrivateServerShowcase.css";

type ServerModel = "pro" | "max";

// Numeric values separated from their units so AnimatedNumber can tween them.
// Strings (cpu) stay plain — no numeric tween for non-numeric specs.
const specs = {
  pro: {
    storageTb: 20,
    speedGbps: 5,
    cpu: "4-8 Core",
    image: products["private-server/base.png"],
  },
  max: {
    storageTb: 40,
    speedGbps: 20,
    cpu: "Ryzen 9 16C",
    image: products["private-server/max-nobg.png"],
  },
} as const;

export default function PrivateServerShowcase() {
  const [model, setModel] = useState<ServerModel>("pro");

  return (
    <section className="private-server-card">
      <div className="private-server-header">
        <div className="private-server-title-block">
          <div className="private-server-kicker">
            <Server size={18} />
            <h3>
              Private Server
            </h3>
          </div>
          <p>
            Your cloud. Your keys.
          </p>
        </div>

        <div className="server-model-toggle" aria-label="Server model">
          <button
            className={model === "pro" ? "active" : ""}
            onClick={() => setModel("pro")}
          >
            Pro
          </button>
          <button
            className={model === "max" ? "active pro" : ""}
            onClick={() => setModel("max")}
          >
            Max
          </button>
        </div>
      </div>

      <div className="server-feature-strip">
        {["Anywhere access", "No static IP", "No monthly fee"].map((label) => (
          <span
            key={label}
          >
            {label}
          </span>
        ))}
      </div>

      <div className="private-server-body">
        <div className="server-copy-stack">
          <div className="server-spec-grid">
            <div className="server-spec-card">
              <div>
                <HardDrive size={15} />
                <span>
                  Storage
                </span>
              </div>
              <strong>
                <AnimatedNumber
                  value={specs[model].storageTb}
                  format={(n) => `${Math.round(n)} TB`}
                />
              </strong>
            </div>

            <div className="server-spec-card">
              <div>
                <Shield size={15} />
                <span>
                  Transfer
                </span>
              </div>
              <strong>
                <AnimatedNumber
                  value={specs[model].speedGbps}
                  format={(n) => `${Math.round(n)} Gbps`}
                />
              </strong>
            </div>

            <div className="server-spec-card server-spec-card--neutral">
              <div>
                <Cpu size={15} />
                <span>
                  Compute
                </span>
              </div>
              <strong>{specs[model].cpu}</strong>
            </div>
          </div>

          <div className="server-benefit-grid">
            <span className="indigo">No Server Room</span>
            <span className="emerald">No Extra Firewall</span>
            <span className="amber">Low Power</span>
            <span className="fuchsia">Tally & ERPs</span>
          </div>

          <div className="server-info-card">
            <div>
              Replaces the usual cloud stack
            </div>
            <div className="server-saas-row">
              <img src={saas["excel.svg"]} alt="Excel" className="h-5 w-5" />
              <img src={saas["gdrive.svg"]} alt="Drive" className="h-5 w-5" />
              <img src={saas["gphotos.svg"]} alt="Photos" className="h-5 w-5" />
              <img src={saas["icloud.svg"]} alt="iCloud" className="h-5 w-5" />
              <span>& more</span>
            </div>
            <div className="server-trademark-note">
              Logos shown to illustrate the services a private server can replace. Not affiliated with, or endorsed by, these companies.
            </div>
          </div>

          <div className="server-info-card server-info-card--compact">
            <div>
              Plug it in. Done.
            </div>
            <div className="server-mini-tags">
              <span>Photos</span>
              <span>Documents</span>
              <span>Tally books</span>
              <span>Family videos</span>
            </div>
          </div>
        </div>

        <div className="server-visual-wrap">
          <div className="server-visual-glow" aria-hidden />
          <div className="server-visual-panel">
            <AnimatePresence mode="wait">
              <motion.img
                key={model}
                src={specs[model].image}
                alt={`Private server ${model}`}
                initial={{ opacity: 0, scale: 0.94, y: 10 }}
                animate={{ opacity: 1, scale: 1, y: 0 }}
                exit={{ opacity: 0, scale: 0.94, y: -10 }}
                transition={{ duration: DURATION_S.slow }}
                className="server-product-image"
              />
            </AnimatePresence>
          </div>
        </div>
      </div>
    </section>
  );
}
