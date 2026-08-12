// src/panels/vault/useStegoBackup.ts
//
// All Stego Backup form state and both backend calls. Split out of
// StegoBackupSection.tsx so the component is only markup: the section had
// grown past the 300-line limit once every state finally had a surface.

import { useCallback, useMemo, useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import useBackend from "../../hooks/useBackend";
import { showSuccess, showError } from "../../utils/toast";
import {
  describeCapacity,
  freeBytesForPath,
  requiredFreeBytes,
  type DriveFreeSpace,
  type SizeUnit,
} from "../../lib/stegoBackup";
import {
  explainStegoFailure,
  validateCreateForm,
  validateExtractForm,
  visibleIssues,
  type StegoFailure,
  type StegoField,
} from "../../lib/stegoBackupValidation";

const VIDEO_FILTER = [{ name: "Video", extensions: ["mp4", "mov", "m4v"] }];
const CONTAINER_FILTER = [{ name: "Encrypted container", extensions: ["hc", "tc"] }];

export type StegoResult = { kind: "ok"; path: string } | { kind: "fail"; failure: StegoFailure } | null;

export function useStegoBackup() {
  const { createStegoMp4, extractStegoMp4, getWipeDriveList, openPath } = useBackend();

  const [carrier, setCarrier] = useState("");
  const [outPath, setOutPath] = useState("");
  const [sizeRaw, setSizeRaw] = useState("20");
  const [sizeUnit, setSizeUnit] = useState<SizeUnit>("M");
  const [password, setPassword] = useState("");
  const [passwordConfirm, setPasswordConfirm] = useState("");
  const [createTried, setCreateTried] = useState(false);
  const [createResult, setCreateResult] = useState<StegoResult>(null);

  const [inPath, setInPath] = useState("");
  const [exOut, setExOut] = useState("");
  const [extractTried, setExtractTried] = useState(false);
  const [extractResult, setExtractResult] = useState<StegoResult>(null);

  const [drives, setDrives] = useState<DriveFreeSpace[] | null>(null);
  const [drivesLoading, setDrivesLoading] = useState(false);
  const [busy, setBusy] = useState<"create" | "extract" | null>(null);

  const loadDrives = useCallback(async () => {
    setDrivesLoading(true);
    try {
      setDrives(await getWipeDriveList());
    } catch {
      // A failed probe must never block the operation — free space stays unknown.
      setDrives([]);
    } finally {
      setDrivesLoading(false);
    }
  }, [getWipeDriveList]);

  const destinationFreeBytes = useMemo(
    () => (drives && outPath ? freeBytesForPath(outPath, drives) : null),
    [drives, outPath],
  );

  // carrierBytes stays null: the webview's fs scope covers only the app's own
  // folders, so nothing here may read the size of a user-chosen video.
  const createVerdict = useMemo(
    () =>
      validateCreateForm({
        carrierPath: carrier,
        outputPath: outPath,
        sizeRaw,
        sizeUnit,
        password,
        passwordConfirm,
        destinationFreeBytes,
        carrierBytes: null,
      }),
    [carrier, outPath, sizeRaw, sizeUnit, password, passwordConfirm, destinationFreeBytes],
  );

  const extractVerdict = useMemo(
    () => validateExtractForm({ inputPath: inPath, outputPath: exOut }),
    [inPath, exOut],
  );

  const createFilled = useMemo(() => {
    const fields: StegoField[] = [];
    if (carrier) fields.push("carrier");
    if (outPath) fields.push("output", "destination");
    if (sizeRaw) fields.push("size");
    if (password) fields.push("password");
    return fields;
  }, [carrier, outPath, sizeRaw, password]);

  const extractFilled = useMemo(() => {
    const fields: StegoField[] = [];
    if (inPath) fields.push("carrier");
    if (exOut) fields.push("output");
    return fields;
  }, [inPath, exOut]);

  const containerMb = createVerdict.containerMb ?? 0;
  const neededBytes = requiredFreeBytes(containerMb, null);

  const runCreate = async () => {
    setCreateTried(true);
    if (!createVerdict.canSubmit || !createVerdict.backendSize) return;
    setBusy("create");
    setCreateResult(null);
    try {
      const res = await createStegoMp4({
        carrierMp4: carrier,
        outputPath: outPath,
        // Carries the unit suffix — a bare number is parsed as 0 MB upstream.
        size: createVerdict.backendSize,
        password,
      });
      if (!res.success) {
        setCreateResult({ kind: "fail", failure: report(res.error, "create") });
        return;
      }
      setCreateResult({ kind: "ok", path: outPath });
      setPassword("");
      setPasswordConfirm("");
      showSuccess("Hidden backup created inside the video");
    } catch (e) {
      setCreateResult({ kind: "fail", failure: report(String(e), "create") });
    } finally {
      setBusy(null);
    }
  };

  const runExtract = async () => {
    setExtractTried(true);
    const target = extractVerdict.normalizedOutputPath;
    if (!extractVerdict.canSubmit || !target) return;
    setBusy("extract");
    setExtractResult(null);
    try {
      const res = await extractStegoMp4({ inputPath: inPath, outputPath: target });
      if (!res.success) {
        setExtractResult({ kind: "fail", failure: report(res.error, "extract") });
        return;
      }
      setExtractResult({ kind: "ok", path: target });
      showSuccess("Hidden container recovered — mount it from the Volumes list");
    } catch (e) {
      setExtractResult({ kind: "fail", failure: report(String(e), "extract") });
    } finally {
      setBusy(null);
    }
  };

  const revealFolder = useCallback(async (path: string) => {
    const normalized = path.replace(/\//g, "\\");
    const cut = normalized.lastIndexOf("\\");
    try {
      await openPath(cut > 0 ? normalized.slice(0, cut) : normalized);
    } catch {
      // Explorer failing to launch is not worth interrupting the user over.
    }
  }, [openPath]);

  return {
    fields: { carrier, outPath, sizeRaw, sizeUnit, password, passwordConfirm, inPath, exOut },
    set: { setCarrier, setOutPath, setSizeRaw, setSizeUnit, setPassword, setPasswordConfirm, setInPath, setExOut },
    busy,
    createResult,
    extractResult,
    createErrors: visibleIssues(createVerdict.errors, createFilled, createTried),
    createWarnings: createVerdict.warnings,
    extractErrors: visibleIssues(extractVerdict.errors, extractFilled, extractTried),
    extractWarnings: extractVerdict.warnings,
    createBlocked: createTried && !createVerdict.canSubmit,
    extractBlocked: extractTried && !extractVerdict.canSubmit,
    capacity: describeCapacity(containerMb, null),
    destinationFreeBytes,
    drivesLoading,
    /** Share of the destination's free space this backup will claim, or null. */
    freeShare: destinationFreeBytes ? neededBytes / destinationFreeBytes : null,
    pickCarrier: async () => {
      const picked = await open({ multiple: false, filters: VIDEO_FILTER });
      if (typeof picked === "string") setCarrier(picked);
    },
    pickOutput: async () => {
      const picked = await save({ defaultPath: "holiday-clip.mp4", filters: VIDEO_FILTER });
      if (picked) {
        setOutPath(picked);
        void loadDrives();
      }
    },
    pickStegoInput: async () => {
      const picked = await open({ multiple: false, filters: VIDEO_FILTER });
      if (typeof picked === "string") setInPath(picked);
    },
    pickContainerOutput: async () => {
      const picked = await save({ defaultPath: "recovered.hc", filters: CONTAINER_FILTER });
      if (picked) setExOut(picked);
    },
    revealFolder,
    runCreate,
    runExtract,
  };
}

/** Translate a failure once, and mirror the headline to the notification bell. */
function report(raw: string | undefined, operation: "create" | "extract"): StegoFailure {
  const failure = explainStegoFailure(raw ?? "", operation);
  showError(failure.headline);
  return failure;
}
