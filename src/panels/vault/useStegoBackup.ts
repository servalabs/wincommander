// State and command boundary for Stego Container Snapshots. An attach copies
// an existing encrypted container as opaque bytes; no password reaches it.
import { useCallback, useMemo, useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import useBackend from "../../hooks/useBackend";
import { showSuccess, showError } from "../../utils/toast";
import { explainStegoFailure, validateAttachForm, validateRestoreFolderForm, visibleIssues, type StegoFailure, type StegoField } from "../../lib/stegoBackupValidation";
import { validateCreateForm } from "../../lib/stegoBackupValidation";
import { type SizeUnit } from "../../lib/stegoBackup";

const VIDEO_FILTER = [{ name: "Video", extensions: ["mp4", "mov", "m4v"] }];
export type StegoResult = { kind: "ok"; path: string } | { kind: "fail"; failure: StegoFailure } | null;
type Operation = "attach" | "restore";
type CommandResult = { success: boolean; error?: string; data?: { outputPath?: string } };

/** Added by the Pro command lane. This keeps the UI's boundary explicit. */
interface SnapshotBackend {
  attachStegoContainer(params: { carrierPath: string; containerPath: string; outputPath?: string; replaceExisting?: boolean }): Promise<CommandResult>;
  restoreStegoContainer(params: { inputPath: string; destinationDir: string; replaceExisting?: boolean }): Promise<CommandResult>;
}

export function useStegoBackup() {
  const backend = useBackend() as ReturnType<typeof useBackend> & SnapshotBackend;
  const [carrierPath, setCarrierPath] = useState("");
  const [containerPath, setContainerPath] = useState("");
  const [outputPath, setOutputPath] = useState("");
  const [replacementConfirmed, setReplacementConfirmed] = useState(false);
  const [attachTried, setAttachTried] = useState(false);
  const [attachResult, setAttachResult] = useState<StegoResult>(null);
  const [restoreVideoPath, setRestoreVideoPath] = useState("");
  const [restoreDestinationDir, setRestoreDestinationDir] = useState("");
  const [restoreTried, setRestoreTried] = useState(false);
  const [restoreResult, setRestoreResult] = useState<StegoResult>(null);
  // Kept for the old, deliberately de-emphasised "empty container" workflow.
  const [legacyCarrierPath, setLegacyCarrierPath] = useState("");
  const [legacyOutputPath, setLegacyOutputPath] = useState("");
  const [legacySizeRaw, setLegacySizeRaw] = useState("20");
  const [legacySizeUnit, setLegacySizeUnit] = useState<SizeUnit>("M");
  const [legacyPassword, setLegacyPassword] = useState("");
  const [legacyPasswordConfirm, setLegacyPasswordConfirm] = useState("");
  const [legacyTried, setLegacyTried] = useState(false);
  const [legacyResult, setLegacyResult] = useState<StegoResult>(null);
  const [busy, setBusy] = useState<Operation | "legacy" | null>(null);

  const attachVerdict = useMemo(() => validateAttachForm({ carrierPath, containerPath, outputPath, replacementConfirmed }), [carrierPath, containerPath, outputPath, replacementConfirmed]);
  const restoreVerdict = useMemo(() => validateRestoreFolderForm({ inputPath: restoreVideoPath, destinationDir: restoreDestinationDir }), [restoreVideoPath, restoreDestinationDir]);
  const legacyVerdict = useMemo(() => validateCreateForm({ carrierPath: legacyCarrierPath, outputPath: legacyOutputPath, sizeRaw: legacySizeRaw, sizeUnit: legacySizeUnit, password: legacyPassword, passwordConfirm: legacyPasswordConfirm, destinationFreeBytes: null, carrierBytes: null }), [legacyCarrierPath, legacyOutputPath, legacySizeRaw, legacySizeUnit, legacyPassword, legacyPasswordConfirm]);
  const filled = (pairs: [StegoField, string][]) => pairs.filter(([, value]) => !!value).map(([field]) => field);

  const runAttach = async () => {
    setAttachTried(true); if (!attachVerdict.canSubmit) return;
    setBusy("attach"); setAttachResult(null);
    try {
      const result = await backend.attachStegoContainer({ carrierPath, containerPath, outputPath: outputPath || undefined, replaceExisting: replacementConfirmed });
      if (!result.success) { setAttachResult({ kind: "fail", failure: report(result.error, "attach") }); return; }
      // The destination is known from the submitted form: a selected output
      // path creates a new video; otherwise the selected carrier is refreshed.
      setAttachResult({ kind: "ok", path: outputPath || carrierPath }); showSuccess("Encrypted container attached to the video");
    } catch (error) { setAttachResult({ kind: "fail", failure: report(String(error), "attach") }); } finally { setBusy(null); }
  };
  const runRestore = async () => {
    setRestoreTried(true); if (!restoreVerdict.canSubmit) return;
    setBusy("restore"); setRestoreResult(null);
    try {
      const result = await backend.restoreStegoContainer({ inputPath: restoreVideoPath, destinationDir: restoreDestinationDir });
      if (!result.success) { setRestoreResult({ kind: "fail", failure: report(result.error, "restore") }); return; }
      if (!result.data?.outputPath) { setRestoreResult({ kind: "fail", failure: report("Restore completed without returning the recovered path", "restore") }); return; }
      setRestoreResult({ kind: "ok", path: result.data.outputPath }); showSuccess("Container recovered with its original filename");
    } catch (error) { setRestoreResult({ kind: "fail", failure: report(String(error), "restore") }); } finally { setBusy(null); }
  };
  const runLegacyCreate = async () => {
    setLegacyTried(true); if (!legacyVerdict.canSubmit || !legacyVerdict.backendSize) return;
    setBusy("legacy"); setLegacyResult(null);
    try {
      const result = await backend.createStegoMp4({ carrierMp4: legacyCarrierPath, outputPath: legacyOutputPath, size: legacyVerdict.backendSize, password: legacyPassword });
      if (!result.success) { setLegacyResult({ kind: "fail", failure: report(result.error, "attach") }); return; }
      setLegacyResult({ kind: "ok", path: legacyOutputPath }); setLegacyPassword(""); setLegacyPasswordConfirm(""); showSuccess("Empty encrypted container created inside the video");
    } catch (error) { setLegacyResult({ kind: "fail", failure: report(String(error), "attach") }); } finally { setBusy(null); }
  };
  const revealFolder = useCallback(async (path: string) => {
    const normalized = path.replace(/\//g, "\\"); const cut = normalized.lastIndexOf("\\");
    try { await backend.openPath(cut > 0 ? normalized.slice(0, cut) : normalized); } catch { /* non-critical */ }
  }, [backend]);
  const pickFile = async (setPath: (value: string) => void, filters?: typeof VIDEO_FILTER) => {
    const picked = await open({ multiple: false, ...(filters ? { filters } : {}) }); if (typeof picked === "string") setPath(picked);
  };
  return {
    fields: { carrierPath, containerPath, outputPath, replacementConfirmed, restoreVideoPath, restoreDestinationDir, legacyCarrierPath, legacyOutputPath, legacySizeRaw, legacySizeUnit, legacyPassword, legacyPasswordConfirm },
    set: { setCarrierPath, setContainerPath, setOutputPath, setReplacementConfirmed, setRestoreVideoPath, setRestoreDestinationDir, setLegacyCarrierPath, setLegacyOutputPath, setLegacySizeRaw, setLegacySizeUnit, setLegacyPassword, setLegacyPasswordConfirm },
    busy, attachResult, restoreResult, legacyResult,
    attachErrors: visibleIssues(attachVerdict.errors, filled([["carrier", carrierPath], ["container", containerPath], ["output", outputPath], ["confirmation", replacementConfirmed ? "yes" : ""]]), attachTried), attachWarnings: attachVerdict.warnings,
    restoreErrors: visibleIssues(restoreVerdict.errors, filled([["carrier", restoreVideoPath], ["destination", restoreDestinationDir]]), restoreTried), restoreWarnings: restoreVerdict.warnings,
    attachBlocked: attachTried && !attachVerdict.canSubmit, restoreBlocked: restoreTried && !restoreVerdict.canSubmit,
    legacyErrors: visibleIssues(legacyVerdict.errors, filled([["carrier", legacyCarrierPath], ["output", legacyOutputPath], ["size", legacySizeRaw], ["password", legacyPassword]]), legacyTried), legacyWarnings: legacyVerdict.warnings, legacyBlocked: legacyTried && !legacyVerdict.canSubmit,
    pickCarrier: () => pickFile(setCarrierPath, VIDEO_FILTER), pickContainer: () => pickFile(setContainerPath),
    pickOutput: async () => { const picked = await save({ defaultPath: "video-backup.mp4", filters: VIDEO_FILTER }); if (picked) setOutputPath(picked); },
    pickRestoreVideo: () => pickFile(setRestoreVideoPath, VIDEO_FILTER),
    pickRestoreDestination: async () => { const picked = await open({ directory: true, multiple: false }); if (typeof picked === "string") setRestoreDestinationDir(picked); },
    pickLegacyCarrier: () => pickFile(setLegacyCarrierPath, VIDEO_FILTER),
    pickLegacyOutput: async () => { const picked = await save({ defaultPath: "empty-container-backup.mp4", filters: VIDEO_FILTER }); if (picked) setLegacyOutputPath(picked); },
    revealFolder, runAttach, runRestore, runLegacyCreate,
  };
}
function report(raw: string | undefined, operation: Operation): StegoFailure {
  const failure = explainStegoFailure(raw ?? "", operation); showError(failure.headline); return failure;
}
