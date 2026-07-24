// src/panels/vault/StegoBackupSection.tsx
//
// Stego backup — hide an encrypted volume inside a playable MP4.
// "Create" makes a container and embeds it in a carrier video; "Extract" pulls
// the container back out so it can be mounted from the Volumes list. Routes to
// the paid Pro handlers Create-StegoMp4 / Extract-StegoMp4.

import { useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { Button, InputGroup, FormGroup } from "@/components/ui/bp";
import SectionCard from "../../components/shared/SectionCard";
import useBackend from "../../hooks/useBackend";
import { showSuccess, showError } from "../../utils/toast";

const VIDEO_FILTER = [{ name: "Video", extensions: ["mp4", "mov", "m4v"] }];

function FilePick({ label, value, onPick }: { label: string; value: string; onPick: () => void }) {
  return (
    <div className="flex items-center gap-2">
      <Button minimal small icon="folder-open" onClick={onPick}>
        {label}
      </Button>
      <span className="truncate text-xs text-[var(--color-text-secondary)]" title={value}>
        {value || "—"}
      </span>
    </div>
  );
}

export default function StegoBackupSection() {
  const { createStegoMp4, extractStegoMp4 } = useBackend();

  const [carrier, setCarrier] = useState("");
  const [outPath, setOutPath] = useState("");
  const [size, setSize] = useState("20");
  const [password, setPassword] = useState("");

  const [inPath, setInPath] = useState("");
  const [exOut, setExOut] = useState("");

  const [busy, setBusy] = useState<"create" | "extract" | null>(null);

  const pickCarrier = async () => {
    const f = await open({ multiple: false, filters: VIDEO_FILTER });
    if (typeof f === "string") setCarrier(f);
  };
  const pickOut = async () => {
    const f = await save({ defaultPath: "backup.mp4", filters: VIDEO_FILTER });
    if (f) setOutPath(f);
  };
  const pickIn = async () => {
    const f = await open({ multiple: false, filters: VIDEO_FILTER });
    if (typeof f === "string") setInPath(f);
  };
  const pickExOut = async () => {
    const f = await save({ defaultPath: "recovered.hc" });
    if (f) setExOut(f);
  };

  const doCreate = async () => {
    if (!carrier || !outPath || !password || !size) {
      showError("Carrier video, output, size and password are all required");
      return;
    }
    setBusy("create");
    try {
      await createStegoMp4({ carrierMp4: carrier, outputPath: outPath, size, password });
      showSuccess("Hidden backup created inside the video");
      setPassword("");
    } catch (e) {
      showError(String(e));
    } finally {
      setBusy(null);
    }
  };

  const doExtract = async () => {
    if (!inPath || !exOut) {
      showError("Stego video and output path are required");
      return;
    }
    setBusy("extract");
    try {
      await extractStegoMp4({ inputPath: inPath, outputPath: exOut });
      showSuccess("Hidden container recovered — mount it from the Volumes list");
    } catch (e) {
      showError(String(e));
    } finally {
      setBusy(null);
    }
  };

  return (
    <SectionCard title="Stego Backup" icon="film">
      <div className="flex flex-col gap-4 p-1">
        <p className="text-xs text-[var(--color-text-secondary)]">
          Hide an encrypted volume inside a normal-looking MP4 that still plays. The video
          carries your backup; only your password unlocks it.
        </p>

        <div className="flex flex-col gap-2">
          <span className="text-sm font-semibold">Create</span>
          <FilePick label="Carrier MP4…" value={carrier} onPick={() => void pickCarrier()} />
          <FilePick label="Output video…" value={outPath} onPick={() => void pickOut()} />
          <FormGroup label="Volume size (MB)">
            <InputGroup value={size} onChange={(e) => setSize(e.currentTarget.value)} />
          </FormGroup>
          <FormGroup label="Password">
            <InputGroup
              type="password"
              value={password}
              onChange={(e) => setPassword(e.currentTarget.value)}
            />
          </FormGroup>
          <Button intent="primary" disabled={busy !== null} onClick={() => void doCreate()}>
            {busy === "create" ? "Creating…" : "Create hidden backup"}
          </Button>
        </div>

        <div className="flex flex-col gap-2 border-t border-[var(--color-border)] pt-3">
          <span className="text-sm font-semibold">Extract</span>
          <FilePick label="Stego MP4…" value={inPath} onPick={() => void pickIn()} />
          <FilePick label="Recover container to…" value={exOut} onPick={() => void pickExOut()} />
          <Button disabled={busy !== null} onClick={() => void doExtract()}>
            {busy === "extract" ? "Extracting…" : "Recover container"}
          </Button>
        </div>
      </div>
    </SectionCard>
  );
}
