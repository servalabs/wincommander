// Stego Container Snapshots: attach, restore, and safely refresh an existing
// encrypted container. The app never asks for the container password here.
import { Button, Checkbox, FormGroup, HTMLSelect, Icon, InputGroup, Popover } from "@/components/ui/bp";
import { useState } from "react";
import SectionCard from "../../components/shared/SectionCard";
import TierGate from "../../components/shared/TierGate";
import type { SizeUnit } from "../../lib/stegoBackup";
import { useStegoBackup } from "./useStegoBackup";
import { BusyBar, FailureCallout, FilePick, INFO, InfoDot, IssueLine, SuccessCallout } from "./StegoBackupParts";
import "./StegoBackupSection.css";

function StegoInfoPopover({ content }: { content: string }) {
  return (
    <Popover
      position="bottom-end"
      popoverClassName="stego-info-popover"
      content={<p>{content}</p>}
    >
      <button type="button" className="stego-info-trigger" aria-label="Stego Backup information">
        <Icon icon="info-sign" size={12} />
      </button>
    </Popover>
  );
}

export default function StegoBackupSection() {
  const stego = useStegoBackup();
  const [showLegacyPassword, setShowLegacyPassword] = useState(false);
  const { fields, set, busy } = stego;
  const attachPath = stego.attachResult?.kind === "ok" ? stego.attachResult.path : null;
  const restorePath = stego.restoreResult?.kind === "ok" ? stego.restoreResult.path : null;
  const legacyPath = stego.legacyResult?.kind === "ok" ? stego.legacyResult.path : null;
  const locked = busy !== null;
  return (
    <SectionCard title="Stego Backup" icon="video" headerRight={<div className="stego-header-actions"><StegoInfoPopover content={INFO.what} /></div>}>
      <div className="stego-section">
        <p className="stego-intro">Attach an existing encrypted container to a normal-looking video that still plays. It is a sealed snapshot: this screen copies the locked container without opening it.</p>
        <TierGate tier="paid" featureLabel="Stego Backup">
          <div className="stego-blocks-row">
            <section className="stego-block" aria-labelledby="stego-attach-title">
              <span id="stego-attach-title" className="stego-block__title">Create or update a video backup</span>
              <p className="stego-intro">Choose a carrier video and the container you already use. Its filename can have any extension or none at all; an extensionless NTFS container is accepted. Its password is never requested or stored.</p>
              <FilePick label="Carrier video or existing backup…" value={fields.carrierPath} onPick={() => void stego.pickCarrier()} onClear={() => set.setCarrierPath("")} disabled={locked} />
              <IssueLine issues={stego.attachErrors} field="carrier" /><IssueLine issues={stego.attachWarnings} field="carrier" tone="warn" />
              <FilePick label="Existing container…" value={fields.containerPath} onPick={() => void stego.pickContainer()} onClear={() => set.setContainerPath("")} disabled={locked} />
              <IssueLine issues={stego.attachErrors} field="container" /><IssueLine issues={stego.attachWarnings} field="container" tone="warn" />
              <FilePick label="Save new backup video as… (optional)" value={fields.outputPath} onPick={() => void stego.pickOutput()} onClear={() => set.setOutputPath("")} disabled={locked} />
              <IssueLine issues={stego.attachErrors} field="output" /><IssueLine issues={stego.attachWarnings} field="output" tone="warn" />
              <Checkbox checked={fields.replacementConfirmed} disabled={locked} label="If no new path is chosen, update the selected existing backup video after verification." onChange={(event) => set.setReplacementConfirmed(event.currentTarget.checked)} />
              <IssueLine issues={stego.attachErrors} field="confirmation" />
              {busy === "attach" && <BusyBar label="Copying and verifying the sealed container; an existing backup is replaced only after this succeeds." />}
              {stego.attachResult?.kind === "fail" && <FailureCallout failure={stego.attachResult.failure} />}
              {attachPath && <SuccessCallout title="Video backup created" path={attachPath} onReveal={() => void stego.revealFolder(attachPath)}>Recover and mount it once before deleting or replacing any other copy.</SuccessCallout>}
              <Button intent="primary" loading={busy === "attach"} disabled={locked || stego.attachBlocked} onClick={() => void stego.runAttach()}>Create or update backup video</Button>
            </section>
            <section className="stego-block stego-block--restore" aria-labelledby="stego-restore-title">
              <span id="stego-restore-title" className="stego-block__title">Restore from a video <InfoDot content={INFO.restore} /></span>
              <p className="stego-intro">Choose a destination folder only. The backup restores the container using the original name saved inside the video.</p>
              <ol className="stego-restore-steps" aria-label="Restore steps"><li>Choose the backup video.</li><li>Choose an empty or safe destination folder.</li><li>Mount the restored original-name container above.</li></ol>
              <FilePick label="Video with a backup…" value={fields.restoreVideoPath} onPick={() => void stego.pickRestoreVideo()} onClear={() => set.setRestoreVideoPath("")} disabled={locked} />
              <IssueLine issues={stego.restoreErrors} field="carrier" /><IssueLine issues={stego.restoreWarnings} field="carrier" tone="warn" />
              <FilePick label="Recover into folder…" value={fields.restoreDestinationDir} onPick={() => void stego.pickRestoreDestination()} onClear={() => set.setRestoreDestinationDir("")} disabled={locked} />
              <IssueLine issues={stego.restoreErrors} field="destination" /><IssueLine issues={stego.restoreWarnings} field="destination" tone="warn" />
              {busy === "restore" && <BusyBar label="Reading the video and recovering the original-name container." />}
              {stego.restoreResult?.kind === "fail" && <FailureCallout failure={stego.restoreResult.failure} />}
              {restorePath && <SuccessCallout title="Container recovered" path={restorePath} onReveal={() => void stego.revealFolder(restorePath)}>Mount it from Encrypted Volumes with its existing password.</SuccessCallout>}
              <Button loading={busy === "restore"} disabled={locked || stego.restoreBlocked} onClick={() => void stego.runRestore()}>Recover container</Button>
            </section>
          </div>
          <details className="stego-details">
            <summary>Advanced: create an empty hidden container</summary>
            <section className="stego-block" aria-label="Create an empty hidden container">
              <p className="stego-intro">This legacy option creates a new empty container. Use “Attach existing container” above for a container that already holds your files.</p>
              <FilePick label="Carrier video…" value={fields.legacyCarrierPath} onPick={() => void stego.pickLegacyCarrier()} onClear={() => set.setLegacyCarrierPath("")} disabled={locked} />
              <IssueLine issues={stego.legacyErrors} field="carrier" />
              <FilePick label="Save video as…" value={fields.legacyOutputPath} onPick={() => void stego.pickLegacyOutput()} onClear={() => set.setLegacyOutputPath("")} disabled={locked} />
              <IssueLine issues={stego.legacyErrors} field="output" />
              <div className="stego-size-row"><FormGroup label="Empty container size"><InputGroup type="number" min={1} value={fields.legacySizeRaw} disabled={locked} onChange={(event) => set.setLegacySizeRaw(event.currentTarget.value)} /></FormGroup><FormGroup label="Unit"><HTMLSelect value={fields.legacySizeUnit} disabled={locked} onChange={(event) => set.setLegacySizeUnit(event.currentTarget.value as SizeUnit)} options={[{ value: "M", label: "MB" }, { value: "G", label: "GB" }, { value: "T", label: "TB" }]} /></FormGroup></div>
              <IssueLine issues={stego.legacyErrors} field="size" />
              <div className="stego-password-row"><FormGroup label="New password"><InputGroup type={showLegacyPassword ? "text" : "password"} value={fields.legacyPassword} autoComplete="new-password" disabled={locked} onChange={(event) => set.setLegacyPassword(event.currentTarget.value)} rightElement={<Button minimal icon={showLegacyPassword ? "eye-off" : "eye-open"} onClick={() => setShowLegacyPassword((current) => !current)} />} /></FormGroup><FormGroup label="Confirm password"><InputGroup type={showLegacyPassword ? "text" : "password"} value={fields.legacyPasswordConfirm} autoComplete="new-password" disabled={locked} onChange={(event) => set.setLegacyPasswordConfirm(event.currentTarget.value)} /></FormGroup></div>
              <IssueLine issues={stego.legacyErrors} field="password" />
              {busy === "legacy" && <BusyBar label="Creating the empty encrypted container and verifying the video." />}
              {stego.legacyResult?.kind === "fail" && <FailureCallout failure={stego.legacyResult.failure} />}
              {legacyPath && <SuccessCallout title="Empty container created" path={legacyPath} onReveal={() => void stego.revealFolder(legacyPath)}>Recover it below before relying on it.</SuccessCallout>}
              <Button loading={busy === "legacy"} disabled={locked || stego.legacyBlocked} onClick={() => void stego.runLegacyCreate()}>Create empty hidden container</Button>
            </section>
          </details>
        </TierGate>
      </div>
    </SectionCard>
  );
}
