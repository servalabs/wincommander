// Compute SHA-256 of wincommander-pro.exe and write it to
// src-tauri/commander-free/scripts/.pro_hash so build.rs can bake
// it into the Free binary via cargo:rustc-env at compile time.
// Called by the `build` script after build:pro:release.
import { createHash } from "crypto";
import { readFileSync, writeFileSync, existsSync } from "fs";
import { join } from "path";

const proExe = join(process.cwd(), "src-tauri", "target", "release", "wincommander-pro.exe");
const outFile = join(process.cwd(), "src-tauri", "commander-free", "scripts", ".pro_hash");

if (!existsSync(proExe)) {
  console.warn(`[hash-pro] ${proExe} not found — skipping hash write`);
  process.exit(0);
}

const data = readFileSync(proExe);
const hash = createHash("sha256").update(data).digest("hex");
writeFileSync(outFile, hash, "utf8");
console.log(`[hash-pro] ${hash}`);
