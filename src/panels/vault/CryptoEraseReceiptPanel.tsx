// The erase receipt, rendered as structured facts instead of one long sentence.
//
// WHY this exists: selective_erase.rs returns nine fields of deliberately
// honest verification data (independent header re-read, escrow honesty
// invariant, FVEK eviction outcome) and the UI used to render exactly one of
// them. escrowWarning in particular is the field that tells a user their
// recovery key is still in Entra/AD and the erase is not final until they
// revoke it there.
import { useState } from "react";
import { Button, Callout, Icon, Tooltip } from "@/components/ui/bp";
import {
  formatReceiptForClipboard,
  receiptEscrowAdvice,
  receiptFacts,
  receiptHeadline,
  receiptTone,
} from "../../lib/cryptoEraseReceipt";
import type { EraseReceipt } from "../../hooks/useBackend";
import "./CryptoEraseReceiptPanel.css";

const TONE_ICON = {
  success: "tick-circle",
  warning: "warning-sign",
  danger: "error",
} as const;

export default function CryptoEraseReceiptPanel({
  receipt,
  at,
  compact = false,
}: {
  receipt: EraseReceipt;
  /** Millisecond timestamp of the erase — shown and copied for the audit trail. */
  at: number;
  /** Inline-in-a-row variant: drops the timestamp row and the copy button. */
  compact?: boolean;
}) {
  const [copied, setCopied] = useState(false);
  const tone = receiptTone(receipt.status);
  const facts = receiptFacts(receipt);
  const escrow = receiptEscrowAdvice(receipt);

  const copy = async () => {
    await navigator.clipboard.writeText(formatReceiptForClipboard(receipt, at)).catch(() => {});
    setCopied(true);
    setTimeout(() => setCopied(false), 1600);
  };

  return (
    <div className={`crypto-erase-receipt crypto-erase-receipt--${tone}`}>
      <div className="crypto-erase-receipt-head">
        <Icon icon={TONE_ICON[tone]} size={14} />
        <span className="crypto-erase-receipt-title">{receiptHeadline(receipt.status)}</span>
        <span className="crypto-erase-receipt-target">{receipt.label}</span>
        {!compact && (
          <>
            <span className="crypto-erase-receipt-time">{new Date(at).toLocaleTimeString()}</span>
            <Button
              minimal
              small
              icon={copied ? "tick" : "duplicate"}
              text={copied ? "Copied" : "Copy receipt"}
              onClick={copy}
            />
          </>
        )}
      </div>

      <div className="crypto-erase-receipt-facts">
        {facts.map((fact) => (
          <div key={fact.label} className={`crypto-erase-fact crypto-erase-fact--${fact.tone}`}>
            <span className="crypto-erase-fact-label">
              {fact.label}
              <Tooltip content={fact.tooltip}>
                <Icon icon="info-sign" size={11} className="crypto-erase-info-icon" />
              </Tooltip>
            </span>
            <span className="crypto-erase-fact-value">{fact.value}</span>
          </div>
        ))}
      </div>

      <p className="crypto-erase-receipt-detail">{receipt.detail}</p>

      {escrow && (
        <Callout intent="danger" title="One step is still outstanding">
          {escrow}
        </Callout>
      )}
    </div>
  );
}
