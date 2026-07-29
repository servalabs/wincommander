// src/panels/vault/StegoBackupSection.tsx
//
// Stego backup — hide an encrypted volume inside a playable MP4.
// "Create" makes a container and appends it to a carrier video; "Restore" pulls
// the container back out so it can be mounted from the Volumes list. Routes to
// the paid Pro handlers Create-StegoMp4 / Extract-StegoMp4.
//
// Rules live in src/lib/stegoBackup*.ts, state in ./useStegoBackup, pieces in
// ./StegoBackupParts — this file is the layout only.

import { Button, FormGroup, HTMLSelect, InputGroup, Tooltip } from "@/components/ui/bp";
import { useState } from "react";
import SectionCard from "../../components/shared/SectionCard";
import TierGate from "../../components/shared/TierGate";
import type { SizeUnit } from "../../lib/stegoBackup";
import { useStegoBackup } from "./useStegoBackup";
import {
  BusyBar,
  CapacityEmpty,
  CapacityPanel,
  FailureCallout,
  FilePick,
  INFO,
  InfoDot,
  IssueLine,
  SuccessCallout,
} from "./StegoBackupParts";
import "./StegoBackupSection.css";

export default function StegoBackupSection() {
  const stego = useStegoBackup();
  const [showPassword, setShowPassword] = useState(false);
  // Destructured so the narrowed result types survive into the reveal callbacks.
  const { fields, set, busy, createErrors, extractErrors, createResult, extractResult } = stego;
  const locked = busy !== null;

  return (
    <SectionCard
      title="Stego Backup"
      icon="video"
      headerRight={
        <div className="stego-header-actions">
          <Tooltip content="Do not upload or re-encode a backup video. Keep the password and the original carrier separately.">
            <Button minimal small icon="warning-sign" aria-label="Important stego backup warning" />
          </Tooltip>
          <InfoDot content={INFO.what} />
        </div>
      }
    >
      <div className="stego-section">
        <p className="stego-intro">
          Hide an encrypted volume inside a normal-looking video that still plays. The video carries
          your backup; only your password opens it.
        </p>

        <TierGate tier="paid" featureLabel="Stego Backup">
          <div className="stego-blocks-row">
            <div className="stego-block">
              <span className="stego-block__title">Create a hidden backup</span>

              <FilePick
                label="Carrier video…"
                value={fields.carrier}
                onPick={() => void stego.pickCarrier()}
                onClear={() => set.setCarrier("")}
                disabled={locked}
              />
              <IssueLine issues={createErrors} field="carrier" />
              <IssueLine issues={stego.createWarnings} field="carrier" tone="warn" />

              <FilePick
                label="Save video as…"
                value={fields.outPath}
                onPick={() => void stego.pickOutput()}
                onClear={() => set.setOutPath("")}
                disabled={locked}
              />
              <IssueLine issues={createErrors} field="output" />
              <IssueLine issues={stego.createWarnings} field="output" tone="warn" />

              <div className="stego-size-row">
                <FormGroup
                  label={
                    <span className="stego-label">
                      Hidden volume size <InfoDot content={INFO.size} />
                    </span>
                  }
                >
                  <InputGroup
                    type="number"
                    min={1}
                    value={fields.sizeRaw}
                    disabled={locked}
                    onChange={(e) => set.setSizeRaw(e.currentTarget.value)}
                  />
                </FormGroup>
                <FormGroup label="Unit">
                  <HTMLSelect
                    value={fields.sizeUnit}
                    disabled={locked}
                    onChange={(e) => set.setSizeUnit(e.currentTarget.value as SizeUnit)}
                    options={[
                      { value: "M", label: "MB" },
                      { value: "G", label: "GB" },
                      { value: "T", label: "TB" },
                    ]}
                  />
                </FormGroup>
              </div>
              <IssueLine issues={createErrors} field="size" />

              {fields.carrier ? (
                <CapacityPanel
                  plan={stego.capacity}
                  freeBytes={stego.destinationFreeBytes}
                  loading={stego.drivesLoading}
                  hasOutput={!!fields.outPath}
                  freeShare={stego.freeShare}
                />
              ) : (
                <CapacityEmpty />
              )}
              <IssueLine issues={createErrors} field="destination" />

              <div className="stego-password-row">
                <FormGroup
                  label={
                    <span className="stego-label">
                      Password <InfoDot content={INFO.password} />
                    </span>
                  }
                  helperText="At least 8 characters. Write it down somewhere safe before you continue."
                >
                  <InputGroup
                    type={showPassword ? "text" : "password"}
                    value={fields.password}
                    autoComplete="new-password"
                    disabled={locked}
                    onChange={(e) => set.setPassword(e.currentTarget.value)}
                    rightElement={
                      <Button
                        minimal
                        icon={showPassword ? "eye-off" : "eye-open"}
                        aria-label={showPassword ? "Hide password" : "Show password"}
                        onClick={() => setShowPassword((prev) => !prev)}
                      />
                    }
                  />
                </FormGroup>
                <FormGroup label="Confirm password">
                  <InputGroup
                    type={showPassword ? "text" : "password"}
                    value={fields.passwordConfirm}
                    autoComplete="new-password"
                    disabled={locked}
                    onChange={(e) => set.setPasswordConfirm(e.currentTarget.value)}
                  />
                </FormGroup>
              </div>
              <IssueLine issues={createErrors} field="password" />

              {busy === "create" && (
                <BusyBar label="Formatting the hidden volume, then rebuilding the video around it. Minutes, not seconds." />
              )}

              {createResult?.kind === "fail" && <FailureCallout failure={createResult.failure} />}
              {createResult?.kind === "ok" && (
                <SuccessCallout
                  title="Hidden backup created"
                  path={createResult.path}
                  onReveal={() => void stego.revealFolder(createResult.path)}
                >
                  Test it before you delete anything: recover it below, then mount the result.
                </SuccessCallout>
              )}

              <Button
                intent="primary"
                loading={busy === "create"}
                disabled={locked || stego.createBlocked}
                onClick={() => void stego.runCreate()}
              >
                Create hidden backup
              </Button>
            </div>

            <div className="stego-block stego-block--restore">
              <span className="stego-block__title">
                Restore from a video <InfoDot content={INFO.restore} />
              </span>
              <p className="stego-intro">
                Copies the hidden container back out as a file. This step needs no password — you enter
                it when you mount the container from the Volumes list above.
              </p>

              <ol className="stego-restore-steps" aria-label="Restore steps">
                <li>Choose the backup video.</li>
                <li>Pick a new location for the container.</li>
                <li>Mount it from Encrypted Volumes.</li>
              </ol>

              <FilePick
                label="Video with a backup…"
                value={fields.inPath}
                onPick={() => void stego.pickStegoInput()}
                onClear={() => set.setInPath("")}
                disabled={locked}
              />
              <IssueLine issues={extractErrors} field="carrier" />

              <FilePick
                label="Recover container to…"
                value={fields.exOut}
                onPick={() => void stego.pickContainerOutput()}
                onClear={() => set.setExOut("")}
                disabled={locked}
              />
              <IssueLine issues={extractErrors} field="output" />
              <IssueLine issues={stego.extractWarnings} field="output" tone="warn" />

              {busy === "extract" && <BusyBar label="Reading the video and copying the hidden container out." />}

              {extractResult?.kind === "fail" && <FailureCallout failure={extractResult.failure} />}
              {extractResult?.kind === "ok" && (
                <SuccessCallout
                  title="Container recovered"
                  path={extractResult.path}
                  onReveal={() => void stego.revealFolder(extractResult.path)}
                >
                  Mount it from the Volumes list above, with the password you used when you created it.
                </SuccessCallout>
              )}

              <Button
                loading={busy === "extract"}
                disabled={locked || stego.extractBlocked}
                onClick={() => void stego.runExtract()}
              >
                Recover container
              </Button>
            </div>
          </div>
        </TierGate>
      </div>
    </SectionCard>
  );
}
