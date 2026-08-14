import { motion } from "framer-motion";
import ContingencySystemSimulation from "./ContingencySystemSimulation";
import TravelHub from "./TravelHub";
import PrivatePhoneSimulation from "./PrivatePhoneSimulation";
import PrivateServerShowcase from "./PrivateServerShowcase";
import "./MoreProductsView.css";

const containerVariants = {
  hidden: { opacity: 0 },
  show: {
    opacity: 1,
    transition: { staggerChildren: 0.08, delayChildren: 0.05 },
  },
};

const itemVariants = {
  hidden: { opacity: 0, y: 18 },
  show: {
    opacity: 1,
    y: 0,
    transition: { duration: 0.45, ease: [0.16, 1, 0.3, 1] as [number, number, number, number] },
  },
};

export default function MoreProductsView() {
  return (
    <div className="more-products-shell">
      <div className="more-products-grid-bg" aria-hidden />

      <motion.div
        className="more-products-inner"
        variants={containerVariants}
        initial="hidden"
        animate="show"
      >
        {/* Hero — site-style headline with animated underline */}
        <motion.header className="mp-hero" variants={itemVariants}>
          <div className="mp-eyebrow">
            <span className="mp-eyebrow-dot" />
            <span>PRIVATE INFRASTRUCTURE</span>
          </div>
          <h1 className="mp-hero-title">
            Privacy You Own.<br />
            <span className="mp-hero-accent">On Your Terms.</span>
          </h1>
          <motion.span
            className="mp-hero-rule"
            initial={{ scaleX: 0 }}
            animate={{ scaleX: 1 }}
            transition={{ duration: 0.9, ease: [0.16, 1, 0.3, 1] as [number, number, number, number], delay: 0.25 }}
          />
        </motion.header>

        {/* Row 1 — Server + Phone */}
        <motion.div className="mp-row mp-row-2-1" variants={itemVariants}>
          <ProductFrame>
            <PrivateServerShowcase />
          </ProductFrame>
          <ProductFrame>
            <PrivatePhoneSimulation />
          </ProductFrame>
        </motion.div>

        {/* Row 2 — Travel Hub + Contingency */}
        <motion.div className="mp-row mp-row-1-2" variants={itemVariants}>
          <ProductFrame>
            <TravelHub />
          </ProductFrame>
          <ProductFrame>
            <ContingencySystemSimulation />
          </ProductFrame>
        </motion.div>
      </motion.div>
    </div>
  );
}

function ProductFrame({ children }: { children: React.ReactNode }) {
  return (
    <div className="mp-frame">
      <div className="mp-frame-inner">{children}</div>
    </div>
  );
}
