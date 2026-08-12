import { Dialog, Button, Icon, Spinner } from "@/components/ui/bp";
import { useCallback, useEffect, useState } from "react";
import useBackend from "../../hooks/useBackend";
import { useTheme } from "../../context/ThemeContext";
import type { VolumeInfo } from "../../hooks/useBackend";
import './VolumePropertiesDialog.css';

interface VolumePropertiesDialogProps {
  isOpen: boolean;
  onClose: () => void;
  letter: string;
  path: string | null;
  type: string;
}

function VolumePropertiesDialog({ isOpen, onClose, letter, path, type }: VolumePropertiesDialogProps) {
  const { theme } = useTheme();
  const { getVolumeInfo } = useBackend();
  const [info, setInfo] = useState<VolumeInfo | null>(null);
  const [loading, setLoading] = useState(false);
  const [errMsg, setErrMsg] = useState<string | null>(null);

  const loadVolumeInfo = useCallback(() => {
    if (!isOpen) return;
    setInfo(null);
    setErrMsg(null);
    setLoading(true);
    getVolumeInfo(letter)
      .then(res => {
        if (res?.success && res.data) setInfo(res.data);
        else setErrMsg(res?.error || "Could not retrieve volume information.");
      })
      .catch(e => setErrMsg(e?.message || "Unknown error"))
      .finally(() => setLoading(false));
  }, [getVolumeInfo, isOpen, letter]);

  useEffect(() => {
    loadVolumeInfo();
  }, [loadVolumeInfo]);

  const safeLetter = letter.replace(/:\\?$/, "") + ":\\";

  const rows: { label: string; value: string }[] = info
    ? [
      { label: "Drive Letter", value: safeLetter },
      { label: "Container Path", value: path || "Device-hosted" },
      { label: "Volume Type", value: type === "Hidden" ? "Hidden" : "Standard" },
      { label: "Size", value: info.size || "—" },
      { label: "Filesystem", value: info.filesystem || "—" },
      { label: "Algorithm", value: info.encryption || "—" },
      { label: "Mode", value: info.mode || "—" },
      { label: "Read Only", value: info.readOnly ? "Yes" : "No" },
    ]
    : [];

  return (
    <Dialog
      isOpen={isOpen}
      onClose={onClose}
      className={`props-dialog ${theme === "light" ? "light" : ""}`}
      backdropProps={{
        style: {
          backgroundColor: theme === "light" ? "rgba(0,0,0,0.4)" : "rgba(0,0,0,0.7)",
          backdropFilter: "blur(4px)",
        },
      }}
    >
      <div className="props-header">
        <span className="props-title">VOLUME PROPERTIES</span>
        <span className="props-drive">{safeLetter}</span>
      </div>

      <div className="props-body">
        {loading && (
          <div className="props-loading" role="status" aria-busy="true">
            <Spinner size={20} />
            <span>Loading…</span>
          </div>
        )}
        {errMsg && !loading && (
          <div className="props-error" role="alert">
            <Icon icon="warning-sign" />
            <span>{errMsg}</span>
          </div>
        )}
        {!loading && !errMsg && (
          <div className="props-table">
            {rows.map(row => (
              <div key={row.label} className="props-row">
                <span className="props-key">{row.label}</span>
                <span className="props-val">{row.value}</span>
              </div>
            ))}
          </div>
        )}
      </div>

      <div className="props-footer">
        <Button text="CLOSE" minimal className="modal-cancel-btn" onClick={onClose} />
      </div>
    </Dialog>
  );
}

export default VolumePropertiesDialog;
