import { useState, useSyncExternalStore } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { Button, Callout, Checkbox } from "@/components/ui/bp";
import SectionCard from "../../components/shared/SectionCard";
import TierGate from "../../components/shared/TierGate";
import useBackend from "../../hooks/useBackend";
import { backupTask } from "../../lib/backupRecoveryTask";
import { FilePick } from "./StegoBackupParts";
import "./BackupRecoverySection.css";

export default function BackupRecoverySection() {
  const backend = useBackend();
  const task = useSyncExternalStore(backupTask.subscribe, backupTask.getSnapshot);
  const [source, setSource] = useState("");
  const [copyParent, setCopyParent] = useState("");
  const [copy, setCopy] = useState("");
  const [generation, setGeneration] = useState("");
  const [restoreParent, setRestoreParent] = useState("");
  const [legacy, setLegacy] = useState(false);
  const [pickerError, setPickerError] = useState("");
  const busy = task.state === "running";
  const pick = async (setPath: (path: string) => void, directory: boolean) => {
    setPickerError("");
    try {
      const path = await open({ directory, multiple: false });
      if (typeof path === "string") setPath(path);
    } catch { setPickerError("The file picker is unavailable. Try selecting the location again."); }
  };
  const field = (label: string, value: string, setPath: (path: string) => void, directory = true) => <FilePick label={label} value={value} disabled={busy} onClear={() => setPath("")} onPick={() => void pick(setPath, directory)} />;
  return <SectionCard title="Encrypted backup" icon="duplicate">
    <p>Copy a closed encrypted container to storage you already use, or recover a retained copy. Existing Windows share credentials are reused.</p>
    <TierGate tier="paid" featureLabel="Encrypted backup recovery">
      <div className="backup-recovery-grid">
        <fieldset disabled={busy}>
          <legend>Copy a closed container</legend>
          <p>Normally dismount it first. Select its backing file, not a file inside the mounted plaintext drive. No password is requested.</p>
          {field("Existing container…", source, setSource, false)}
          {field("Backup location…", copyParent, setCopyParent)}
          <p>A new copy folder is created under the selected local, mapped-drive or network-share location. Existing copies are never overwritten.</p>
          <Button intent="primary" disabled={busy || !source || !copyParent} onClick={() => void backupTask.run("copied", () => backend.exportEncryptedBackup(source, copyParent), copyParent)}>Copy and check container</Button>
        </fieldset>
        <fieldset disabled={busy}>
          <legend>Check a retrieved copy</legend>
          {field("Exported copy folder…", copy, setCopy)}
          <p>Choose the folder containing container.bin and container-copy-v1.json. Then mount container.bin read-only with Encrypted Volumes above and your existing credentials.</p>
          <Button disabled={busy || !copy} onClick={() => void backupTask.run("checked", () => backend.verifyEncryptedBackup(copy))}>Check copy</Button>
        </fieldset>
        <fieldset disabled={busy}>
          <legend>Restore backed-up files</legend>
          {field("Generation folder in the mounted copy…", generation, setGeneration)}
          {field("Restore location…", restoreParent, setRestoreParent)}
          <Checkbox checked={legacy} onChange={(event) => setLegacy(event.currentTarget.checked)} label="Legacy recovery: original per-file hashes are unavailable" />
          <p>A new folder is created. Restored files are plaintext: choose a private or encrypted location. Empty directories, ACLs, timestamps and alternate streams are not restored.</p>
          <Button disabled={busy || !generation || !restoreParent} onClick={() => void backupTask.run("restored", () => backend.restoreBackupGeneration(generation, restoreParent, legacy), restoreParent)}>Check and restore files</Button>
        </fieldset>
      </div>
      {pickerError && <div role="alert"><Callout intent="warning">{pickerError}</Callout></div>}
      {busy && <div role="status" className="backup-recovery-status">{task.action === "copied" ? "Copying and checking the container…" : task.action === "restored" ? "Checking and restoring file contents…" : "Checking the retrieved copy…"} Keep WinCommander open. Other protection commands remain available; closing the app can leave an incomplete copy.</div>}
      {(task.state === "failed" || task.state === "unknown") && <div role="alert"><Callout intent="warning">{task.message}{task.destinationParent && <p className="backup-recovery-path">Selected destination parent: {task.destinationParent}</p>}</Callout></div>}
      {task.state === "done" && task.result && <div role="status"><Callout title={task.action === "restored" ? "File contents restored and checked" : "Container bytes checked"}>
        <p>{task.result.bytes} bytes checked.{task.result.files !== undefined ? ` ${task.result.files} files restored.` : ""} {task.result.legacy ? "Legacy recovery checked current bytes only; original hashes were unavailable." : ""}</p>
        {task.destination && <p className="backup-recovery-path">Destination: {task.destination}</p>}
        {task.action === "restored" && <p>Destination contains plaintext file contents. Original filesystem metadata was not restored.</p>}
      </Callout></div>}
      <p className="backup-recovery-limits">Checks establish byte consistency, not encryption, mountability, cloud upload, remote durability or protected retention. Keep existing recovery credentials separately. Failed copies are retained; retries require a new destination.</p>
    </TierGate>
  </SectionCard>;
}
