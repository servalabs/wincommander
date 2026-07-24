import { readFile, writeFile, readdir, stat } from "node:fs/promises";
import { join, relative } from "node:path";
import { createCipheriv, randomBytes, createHash } from "node:crypto";

// Paths after Phase 6b — the encrypted-modules tree moved with the
// commander-free crate from src-tauri/scripts to
// src-tauri/commander-free/scripts/.
const MODULES_DIR = join(process.cwd(), "src-tauri", "commander-free", "scripts", "modules");
const CORE_DIR = join(process.cwd(), "src-tauri", "commander-free", "scripts", "core");
const BUILD_SALT_PATH = join(process.cwd(), "src-tauri", "commander-free", "scripts", ".build_salt");

// KT: Each encryption run generates a fresh 32-byte random salt.
// The salt is saved to .build_salt so build.rs can embed it (XOR-obfuscated) into the binary.
// This means every build uses a unique key — extracting the key from one build
// does not compromise modules encrypted for a different build.
async function deriveKey(): Promise<Buffer> {
  const salt = randomBytes(32);
  await writeFile(BUILD_SALT_PATH, salt);
  console.log("🔑 Generated per-build encryption salt → .build_salt");
  return createHash("sha256").update(salt).digest();
}

// Encrypt a single module with AES-256-GCM
async function encryptModule(modulePath: string, outputPath: string, key: Buffer) {
  const iv = randomBytes(12); // GCM standard IV size
  const cipher = createCipheriv("aes-256-gcm", key, iv);

  const content = await readFile(modulePath);
  const encrypted = Buffer.concat([
    cipher.update(content),
    cipher.final()
  ]);

  const authTag = cipher.getAuthTag();

  // Output format: [IV(12 bytes) | AuthTag(16 bytes) | Encrypted Data]
  const output = Buffer.concat([iv, authTag, encrypted]);
  await writeFile(outputPath, output);

  console.log(`  ✅ ${relative(process.cwd(), modulePath)} -> ${relative(process.cwd(), outputPath)}`);
}

// Recursively find script files by extension
async function findScriptFiles(dir: string, extensions: string[]): Promise<string[]> {
  const files: string[] = [];
  const entries = await readdir(dir, { withFileTypes: true });

  for (const entry of entries) {
    const fullPath = join(dir, entry.name);
    if (entry.isDirectory()) {
      files.push(...await findScriptFiles(fullPath, extensions));
    } else if (entry.isFile() && extensions.some(ext => entry.name.endsWith(ext))) {
      files.push(fullPath);
    }
  }

  return files;
}

async function main() {
  console.log("🔐 AES-256-GCM Module Encryption");
  console.log("================================\n");

  const key = await deriveKey();

  // Encrypt core modules
  console.log("📦 Encrypting core modules...");
  const coreFiles = await findScriptFiles(CORE_DIR, [".ps1"]);
  for (const file of coreFiles) {
    const ext = file.endsWith('.py') ? '.py' : '.ps1';
    const outputPath = file.replace(ext, '.enc');
    await encryptModule(file, outputPath, key);
  }

  // Encrypt feature modules
  console.log("\n📦 Encrypting feature modules...");
  // Only encrypt PowerShell modules to avoid overwriting .enc with helper .py files.
  const moduleFiles = await findScriptFiles(MODULES_DIR, [".ps1"]);
  for (const file of moduleFiles) {
    const ext = file.endsWith('.py') ? '.py' : '.ps1';
    const outputPath = file.replace(ext, '.enc');
    await encryptModule(file, outputPath, key);
  }

  console.log(`\n✨ Successfully encrypted ${coreFiles.length + moduleFiles.length} modules`);
}

main().catch(error => {
  console.error("❌ Encryption failed:", error);
  process.exit(1);
});
