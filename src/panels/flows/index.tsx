// Flows panel (v2) — the paid, Pro-backed automation surface.
//
// "When this happens, do that." Rules are authored here, persisted to
// `settings.app.proFlows`, and evaluated by the Pro flow engine. This panel is
// a thin, focused client over the `flow_bridge` Tauri commands (see
// useFlowsV2). The old React-Flow node canvas + the broken local engine it
// drove are gone.

import { useMemo, useState } from "react";
import { motion, AnimatePresence } from "framer-motion";
import PanelHeader from "@/components/shared/PanelHeader";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { Icon } from "@/components/ui/icon";
import { Dialog, DialogContent, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import useEntitlements from "@/hooks/useEntitlements";
import { useFlowsV2 } from "@/hooks/useFlowsV2";
import { useSettingsQuery } from "@/hooks/queries/useSettingsQuery";
import RuleEditor from "./RuleEditor";
import { buildFlowSettingOptions } from "./settingsCatalog";
import {
  RULE_TEMPLATES,
  emptyRule,
  isFleetLocked,
  ruleSummary,
  type Rule,
} from "./rules";
import "./index.css";

function TemplateGrid({ onSelect }: { onSelect: (rule: Rule) => void }) {
  return (
    <div className="flows-templates__grid">
      {RULE_TEMPLATES.map((t) => (
        <button
          key={t.key}
          type="button"
          className="flows-template"
          onClick={() => onSelect(t.build())}
        >
          <span className="flows-template__name">{t.name}</span>
          <span className="flows-template__blurb">{t.blurb}</span>
          <span className="flows-template__cta">
            <Icon icon="plus" size={12} /> Use template
          </span>
        </button>
      ))}
    </div>
  );
}

export default function FlowsPanel() {
  const { hasPaid } = useEntitlements();
  const { rules, commands, loading, error, log, saveRule, deleteRule, setEnabled, fireNow } = useFlowsV2(hasPaid);
  const { data: settings } = useSettingsQuery();
  const [editing, setEditing] = useState<Rule | null>(null);
  const [showTemplates, setShowTemplates] = useState(false);
  const settingOptions = useMemo(() => buildFlowSettingOptions(settings), [settings]);

  const chooseTemplate = (rule: Rule) => {
    setEditing(rule);
    setShowTemplates(false);
  };

  const sorted = useMemo(
    () => [...rules].sort((a, b) => Number(isFleetLocked(a)) - Number(isFleetLocked(b))),
    [rules],
  );

  const openLicenseGate = () => window.dispatchEvent(new CustomEvent("open-license-gate"));

  return (
    <div className="flows-panel">
      <div className="flows-panel__top">
        <PanelHeader
          title="Flows"
          description="Automate WinCommander: when something happens, do something. Telemetry flips on → turn location off. Privacy Shield sees a shoulder-surfer → cut the camera."
          panelId="flows"
        />
        {hasPaid && (
          <div className="flex items-center gap-2">
            <Button variant="outline" onClick={() => setShowTemplates(true)}>
              <Icon icon="layout-grid" size={14} /> Browse templates
            </Button>
            <Button className="flows-panel__new" onClick={() => setEditing(emptyRule())}>
              <Icon icon="plus" size={14} /> New flow
            </Button>
          </div>
        )}
      </div>

      {!hasPaid ? (
        <div className="flows-gate">
          <Icon icon="data-lineage" size={28} />
          <h2>Flows is a Pro feature</h2>
          <p>
            Build if-this-then-that automations across every WinCommander control, run them
            standalone, and push locked rule sets to a fleet. Unlock with a Pro license.
          </p>
          <Button onClick={openLicenseGate}>Unlock Pro</Button>
        </div>
      ) : (
        <>
          {rules.length === 0 && !loading && (
            <div className="flows-templates">
              <p className="flows-templates__lead">Start from a template:</p>
              <TemplateGrid onSelect={chooseTemplate} />
            </div>
          )}

          {error && <div className="flows-error">{error}</div>}

          <div className="flows-list">
            <AnimatePresence initial={false}>
              {sorted.map((rule) => {
                const { when, then } = ruleSummary(rule);
                const locked = isFleetLocked(rule);
                return (
                  <motion.div
                    key={rule.id}
                    layout
                    initial={{ opacity: 0, y: 6 }}
                    animate={{ opacity: 1, y: 0 }}
                    exit={{ opacity: 0, y: -6 }}
                    transition={{ duration: 0.18 }}
                    className={`flow-card${rule.enabled ? " flow-card--on" : ""}`}
                  >
                    <div className="flow-card__head">
                      <Switch
                        checked={rule.enabled}
                        disabled={locked}
                        onCheckedChange={(on) => void setEnabled(rule.id, on)}
                        aria-label={`Enable ${rule.name}`}
                      />
                      <span className="flow-card__name">{rule.name}</span>
                      {locked && (
                        <span className="flow-chip flow-chip--fleet">
                          <Icon icon="lock" size={11} /> Fleet
                        </span>
                      )}
                      {rule.riskLevel && rule.riskLevel !== "low" && (
                        <span className={`flow-chip flow-chip--risk-${rule.riskLevel}`}>{rule.riskLevel}</span>
                      )}
                      <span className="flow-card__actions">
                        <button type="button" title="Run now" onClick={() => void fireNow(rule.id)}>
                          <Icon icon="play" size={13} />
                        </button>
                        <button type="button" title="Edit" onClick={() => setEditing(rule)}>
                          <Icon icon="edit" size={13} />
                        </button>
                        {!locked && (
                          <button type="button" title="Delete" onClick={() => void deleteRule(rule.id)}>
                            <Icon icon="trash" size={13} />
                          </button>
                        )}
                      </span>
                    </div>
                    <div className="flow-card__flow">
                      <span className="flow-card__seg flow-card__seg--when">
                        <span className="flow-card__seg-label">When</span> {when}
                      </span>
                      <Icon icon="arrow-right" size={13} className="flow-card__arrow" />
                      <span className="flow-card__seg flow-card__seg--then">
                        <span className="flow-card__seg-label">Do</span> {then}
                      </span>
                    </div>
                  </motion.div>
                );
              })}
            </AnimatePresence>
          </div>

          {log.length > 0 && (
            <div className="flows-activity">
              <h3 className="flows-activity__title">
                <Icon icon="history" size={13} /> Recent activity
              </h3>
              <ul className="flows-activity__list">
                {log.map((entry) => (
                  <li key={entry.id} className={`flows-activity__item flows-activity__item--${entry.kind}`}>
                    <span className="flows-activity__kind">{entry.kind}</span>
                    <span className="flows-activity__msg">{entry.message}</span>
                  </li>
                ))}
              </ul>
            </div>
          )}
        </>
      )}

      {showTemplates && (
        <Dialog open onOpenChange={(o) => !o && setShowTemplates(false)}>
          <DialogContent className="flow-editor">
            <DialogHeader>
              <DialogTitle>Browse templates</DialogTitle>
            </DialogHeader>
            <div className="flow-editor__body">
              <TemplateGrid onSelect={chooseTemplate} />
            </div>
          </DialogContent>
        </Dialog>
      )}

      {editing && (
        <RuleEditor
          rule={editing}
          commands={commands}
          settingOptions={settingOptions}
          onSave={saveRule}
          onClose={() => setEditing(null)}
        />
      )}
    </div>
  );
}
