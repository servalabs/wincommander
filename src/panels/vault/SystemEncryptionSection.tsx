import { Icon, Spinner } from "@/components/ui/bp";
import { useCallback, useState } from "react";
import useBackend from "../../hooks/useBackend";
import type { SystemEncryptionStatus } from "../../hooks/useBackend";
import './SystemEncryptionSection.css';

interface SystemEncryptionSectionProps {
  installed: boolean;
  /** Compact one-line status row variant for embedding inside another
   *  card (e.g. the Encrypted Volumes card on the Secure Storage panel).
   *  Skips the section title, body card, algorithm/mode meta, and the
   *  progress bar — the owner asked for "just the status" in that slot. */
  compact?: boolean;
}

function SystemEncryptionSection({ installed, compact = false }: SystemEncryptionSectionProps) {
  const { getSystemEncryptionStatus } = useBackend();
  const [status, setStatus] = useState<SystemEncryptionStatus | null>(null);
  const [loading, setLoading] = useState(false);
  const [errMsg, setErrMsg] = useState<string | null>(null);

  const fetchStatus = useCallback(async () => {
    if (!installed) return;
    setLoading(true);
    setErrMsg(null);
    try {
      const res = await getSystemEncryptionStatus();
      if (res?.success && res.data) setStatus(res.data);
      else setErrMsg(res?.error || "Could not determine system drive encryption status.");
    } catch (e: any) {
      setErrMsg(e?.message || "Unknown error");
    } finally {
      setLoading(false);
    }
  }, [getSystemEncryptionStatus, installed]);

  if (!installed) return null;

  const hasStatus = status !== null;
  const isEncrypted = status?.encrypted === true;
  const inProgress = typeof status?.progress === "number" && status.progress < 100 && status.progress > 0;

  // Compact single-line status row for embedding inside the Encrypted
  // Volumes card. Reuses the same backend hook so it stays in sync.
  if (compact) {
    return (
      <div className={`vault-sys-enc-row ${hasStatus ? (isEncrypted ? "is-encrypted" : "is-unprotected") : "is-unknown"}`}>
        <Icon
          icon={loading ? "refresh" : !hasStatus ? "help" : isEncrypted ? "lock" : "unlock"}
          size={12}
        />
        <span className="vault-sys-enc-label">System drive</span>
        <span>·</span>
        <span>
          {loading
            ? "Checking…"
            : errMsg
              ? errMsg
              : !hasStatus
                ? "Not checked"
              : isEncrypted
                ? "Encrypted"
                : "Not encrypted"}
        </span>
        <button
          type="button"
          className="vault-sys-enc-refresh"
          onClick={fetchStatus}
          disabled={loading}
          title="Refresh system drive encryption status"
          aria-label="Refresh system drive encryption status"
        >
          <Icon icon="refresh" size={10} />
        </button>
      </div>
    );
  }

  return (
    <div className="sys-enc-section">
      <div className="sys-enc-header">
        <span className="sys-enc-title">SYSTEM DRIVE</span>
        <button className="sys-enc-refresh" onClick={fetchStatus} disabled={loading} title="Refresh">
          <Icon icon="refresh" size={10} />
        </button>
      </div>

      <div className="sys-enc-body">
        {loading && (
          <div className="sys-enc-loading">
            <Spinner size={14} />
            <span>Checking system drive…</span>
          </div>
        )}

        {errMsg && !loading && (
          <div className="sys-enc-status unknown">
            <Icon icon="warning-sign" size={14} />
            <span>{errMsg}</span>
          </div>
        )}

        {!loading && !errMsg && !status && (
          <div className="sys-enc-status unknown">
            <Icon icon="help" size={14} />
            <span>Not checked. Refresh to read system drive encryption status.</span>
          </div>
        )}

        {!loading && !errMsg && status && (
          <>
            <div className={`sys-enc-status ${isEncrypted ? "protected" : "unprotected"}`}>
              <Icon icon={isEncrypted ? "lock" : "unlock"} size={14} />
              <div className="sys-enc-status-text">
                <span className="sys-enc-status-label">
                  {isEncrypted ? "ENCRYPTED" : "NOT ENCRYPTED"}
                </span>
                <span className="sys-enc-status-sep">·</span>
                <span className="sys-enc-status-desc">
                  {isEncrypted
                    ? "Pre-boot authentication active."
                    : "Physical access risk — data is readable without a password."}
                </span>
              </div>
            </div>

            {inProgress && (
              <div className="sys-enc-progress">
                <div className="sys-enc-progress-label">
                  <span>Encryption in progress</span>
                  <span>{status.progress?.toFixed(1)}%</span>
                </div>
                <div className="sys-enc-progress-bar">
                  <div
                    className="sys-enc-progress-fill"
                    style={{ width: `${status.progress}%` }}
                  />
                </div>
              </div>
            )}

            {status.algorithm && (
              <div className="sys-enc-meta">
                {status.algorithm && <span><span className="meta-label">ALGORITHM</span> {status.algorithm}</span>}
                {status.mode && <span><span className="meta-label">MODE</span> {status.mode}</span>}
              </div>
            )}
          </>
        )}
      </div>
    </div>
  );
}

export default SystemEncryptionSection;
