use std::fs;
use std::path::Path;

// KT: Per-build encryption key derivation.
// encrypt.ts writes a random 32-byte salt to scripts/.build_salt before each build.
// build.rs reads it, XOR-obfuscates it with a compile-time mask, and emits
// generated_key.rs into OUT_DIR so the salt never appears as a plain string in the binary.
fn main() {
    println!("cargo:rerun-if-env-changed=WINCMD_PRO_SHA256_CURRENT");
    println!("cargo:rerun-if-env-changed=WINCMD_PRO_SHA256_PREVIOUS");

    // Licence config is embedded via option_env!() in license.rs (release builds
    // ignore the runtime env, F-2). Declare those vars here so cargo recompiles
    // the crate when they change — otherwise a release built once without them
    // stays "Licensing is not configured" even after you set them on a rebuild.
    println!("cargo:rerun-if-env-changed=WINCMD_LICENSE_API_BASE");
    println!("cargo:rerun-if-env-changed=LICENSE_API_BASE");
    println!("cargo:rerun-if-env-changed=WINCMD_LICENSE_PUBLIC_KEY");
    println!("cargo:rerun-if-env-changed=LICENSE_PUBLIC_KEY_B64");
    println!("cargo:rerun-if-env-changed=TAURI_SIGNING_PUBLIC_KEY");
    println!("cargo:rerun-if-env-changed=WINCMD_APP_ID");

    // Primary source: .pro_hash file written by tools/hash-pro.ts right
    // after build:pro:release. This runs inside beforeBuildCommand so the
    // hash is always the exact binary that will be packaged — no chicken-
    // and-egg between building Pro and hashing it.
    // Fallback: WINCMD_PRO_SHA256_CURRENT env var (manual override).
    let hash_file = Path::new("scripts/.pro_hash");
    println!("cargo:rerun-if-changed=scripts/.pro_hash");
    let file_hash = if hash_file.exists() {
        fs::read_to_string(hash_file)
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    } else {
        None
    };

    let current_hash = file_hash.or_else(|| {
        std::env::var("WINCMD_PRO_SHA256_CURRENT")
            .ok()
            .filter(|s| !s.trim().is_empty())
    });
    if let Some(h) = current_hash {
        println!("cargo:rustc-env=WINCMD_PRO_SHA256_CURRENT={}", h.trim());
    }

    if let Ok(v) = std::env::var("WINCMD_PRO_SHA256_PREVIOUS") {
        if !v.trim().is_empty() {
            println!("cargo:rustc-env=WINCMD_PRO_SHA256_PREVIOUS={}", v.trim());
        }
    }

    // Production builds use highestAvailable: standard users keep their own
    // medium-integrity token while administrators can elevate when needed.
    // Development builds remain asInvoker so `tauri dev` and the CLI never
    // prompt solely because they were launched from a developer terminal.
    // Privileged commands still enforce their existing administrator checks.
    let release_manifest = include_str!("app.manifest");
    const HIGHEST_AVAILABLE_LEVEL: &str = r#"level="highestAvailable""#;
    assert!(
        release_manifest.contains(HIGHEST_AVAILABLE_LEVEL),
        "the release manifest must retain requestedExecutionLevel=highestAvailable"
    );
    let is_development_profile = std::env::var("PROFILE").as_deref() == Ok("debug");
    println!("cargo:rustc-check-cfg=cfg(wincommander_dev_profile)");
    if is_development_profile {
        println!("cargo:rustc-cfg=wincommander_dev_profile");
    }
    let development_manifest;
    let app_manifest = if is_development_profile {
        development_manifest = release_manifest.replacen(HIGHEST_AVAILABLE_LEVEL, r#"level="asInvoker""#, 1);
        development_manifest.as_str()
    } else {
        release_manifest
    };
    println!("cargo:rerun-if-changed=app.manifest");

    let mut windows = tauri_build::WindowsAttributes::new();
    windows = windows.app_manifest(app_manifest);
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
    {
        // The Cargo lib test harness does not receive Tauri's app manifest, but
        // Tauri's dialog stack imports TaskDialogIndirect from Common Controls v6.
        println!("cargo:rustc-link-arg=/MANIFESTDEPENDENCY:type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'");
    }

    // --- Per-build encryption key generation ---
    let salt_path = Path::new("scripts/.build_salt");
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR not set");

    if salt_path.exists() {
        let salt_bytes = fs::read(salt_path).expect("Failed to read .build_salt");
        assert!(
            salt_bytes.len() == 32,
            ".build_salt must be exactly 32 bytes"
        );

        // XOR mask to prevent plain salt bytes from appearing in the binary.
        // The mask is arbitrary; it only needs to be consistent between build.rs and runtime.
        let xor_mask: [u8; 32] = [
            0xA3, 0x5F, 0x1B, 0x7E, 0xC4, 0x92, 0xD8, 0x46, 0x0D, 0xE1, 0x3A, 0x69, 0xB7, 0x54,
            0xF0, 0x28, 0x8C, 0x73, 0x2E, 0x95, 0x41, 0xDA, 0x06, 0xBB, 0x67, 0x1F, 0xE8, 0x50,
            0x9D, 0xA6, 0x34, 0xC1,
        ];

        let obfuscated: Vec<u8> = salt_bytes
            .iter()
            .zip(xor_mask.iter())
            .map(|(s, m)| s ^ m)
            .collect();

        // Emit Rust source with the obfuscated salt and the same mask for deobfuscation
        let generated = format!(
            "// Auto-generated by build.rs — do not edit\n\
             pub const OBFUSCATED_SALT: [u8; 32] = {:?};\n\
             pub const XOR_MASK: [u8; 32] = {:?};\n",
            obfuscated.as_slice(),
            xor_mask,
        );

        let gen_path = Path::new(&out_dir).join("generated_key.rs");
        fs::write(&gen_path, generated).expect("Failed to write generated_key.rs");

        // Re-run build.rs if the salt file changes
        println!("cargo:rerun-if-changed=scripts/.build_salt");
    } else {
        // Fallback: if no .build_salt exists (first clone, CI without encrypt step),
        // emit a dummy that will fail decryption with a clear error.
        let generated = "// Auto-generated by build.rs — no .build_salt found\n\
             pub const OBFUSCATED_SALT: [u8; 32] = [0u8; 32];\n\
             pub const XOR_MASK: [u8; 32] = [0u8; 32];\n";
        let gen_path = Path::new(&out_dir).join("generated_key.rs");
        fs::write(&gen_path, generated).expect("Failed to write generated_key.rs");
        println!("cargo:rerun-if-changed=scripts/.build_salt");
        println!("cargo:warning=No .build_salt found — modules will fail to decrypt. Run `bun run encrypt-backend` first.");
    }

    tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows))
        .expect("failed to run build script");
}
