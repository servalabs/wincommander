use futures::StreamExt;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;

mod manifest;

use manifest::{
    fetch_manifest, validate_manifest, verify_manifest, InvestigatorArtifact, InvestigatorManifest,
    SignedManifestEnvelope, INVESTIGATOR_BINARY_NAME, PRO_SIDECAR_NAME,
};

const INVESTIGATOR_DIR_NAME: &str = "investigator";
const INSTALL_METADATA_NAME: &str = "investigator-install.json";
const DOWNLOAD_TIMEOUT_SECS: u64 = 300;

#[derive(serde::Deserialize, serde::Serialize)]
struct InstallMetadata {
    schema_version: u8,
    signed_manifest: SignedManifestEnvelope,
}

#[derive(serde::Serialize)]
struct InvestigatorInstallStatus {
    installed: bool,
    install_dir: String,
    executable_path: String,
    version: Option<String>,
}

fn require_investigator_entitlement() -> Result<(), String> {
    if crate::license::has_entitlement("advanced") {
        Ok(())
    } else {
        Err("entitlement:An active Investigator subscription is required.".to_string())
    }
}

fn install_dir() -> Result<PathBuf, String> {
    let dir = crate::paths::machine_data_dir()?.join(INVESTIGATOR_DIR_NAME);
    std::fs::create_dir_all(&dir).map_err(|e| format!("disk:create install directory: {}", e))?;
    crate::paths::harden_dir_acl(&dir);
    Ok(dir)
}

fn metadata_path() -> Result<PathBuf, String> {
    Ok(install_dir()?.join(INSTALL_METADATA_NAME))
}

fn artifact_path(artifact: &InvestigatorArtifact) -> Result<PathBuf, String> {
    if !matches!(
        artifact.name.as_str(),
        INVESTIGATOR_BINARY_NAME | PRO_SIDECAR_NAME
    ) {
        return Err("validation:unexpected artifact name".to_string());
    }
    Ok(install_dir()?.join(&artifact.name))
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("disk:read {}: {}", path.display(), e))?;
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{:02x}", byte))
        .collect())
}

fn installed_metadata() -> Option<InstallMetadata> {
    let path = metadata_path().ok()?;
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

fn install_matches(manifest: &InvestigatorManifest) -> bool {
    validate_manifest(manifest).is_ok()
        && manifest.artifacts.iter().all(|artifact| {
            artifact_path(artifact)
                .and_then(|path| sha256_file(&path))
                .is_ok_and(|actual| actual.eq_ignore_ascii_case(&artifact.sha256))
        })
}

fn verified_installed_manifest() -> Option<InvestigatorManifest> {
    let metadata = installed_metadata()?;
    if metadata.schema_version != 1 {
        return None;
    }
    let manifest = verify_manifest(&metadata.signed_manifest).ok()?;
    install_matches(&manifest).then_some(manifest)
}

async fn download_artifact(artifact: &InvestigatorArtifact, path: &Path) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(DOWNLOAD_TIMEOUT_SECS))
        .dns_resolver(crate::net::doh_resolver())
        .build()
        .map_err(|e| format!("download:http client: {}", e))?;
    let response = client
        .get(&artifact.url)
        .send()
        .await
        .map_err(|e| format!("download:{} GET failed: {}", artifact.name, e))?;
    if !response.status().is_success() {
        return Err(format!(
            "download:{} returned HTTP {}",
            artifact.name,
            response.status()
        ));
    }
    if response
        .content_length()
        .is_some_and(|size| size != artifact.size)
    {
        return Err(format!(
            "download:{} content length does not match manifest",
            artifact.name
        ));
    }
    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .await
        .map_err(|e| format!("disk:create {}: {}", path.display(), e))?;
    let mut received = 0_u64;
    let mut hasher = Sha256::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("download:{} body read: {}", artifact.name, e))?;
        received = received.saturating_add(chunk.len() as u64);
        if received > artifact.size {
            return Err(format!("download:{} exceeds manifest size", artifact.name));
        }
        hasher.update(&chunk);
        file.write_all(&chunk)
            .await
            .map_err(|e| format!("disk:write {}: {}", path.display(), e))?;
    }
    file.sync_all()
        .await
        .map_err(|e| format!("disk:fsync {}: {}", path.display(), e))?;
    if received != artifact.size {
        return Err(format!(
            "download:{} size does not match manifest",
            artifact.name
        ));
    }
    let actual = hasher
        .finalize()
        .iter()
        .map(|byte| format!("{:02x}", byte))
        .collect::<String>();
    if !actual.eq_ignore_ascii_case(&artifact.sha256) {
        return Err(format!(
            "sha256_mismatch:{} did not match signed manifest",
            artifact.name
        ));
    }
    Ok(())
}

fn replace_file(source: &Path, destination: &Path) -> Result<(), String> {
    if destination.exists() {
        std::fs::remove_file(destination).map_err(|e| {
            format!(
                "disk:replace {}: close Investigator first ({})",
                destination.display(),
                e
            )
        })?;
    }
    std::fs::rename(source, destination)
        .map_err(|e| format!("disk:install {}: {}", destination.display(), e))
}

#[tauri::command]
pub async fn fetch_investigator_manifest() -> Result<serde_json::Value, String> {
    require_investigator_entitlement()?;
    let (_, manifest) = fetch_manifest().await?;
    serde_json::to_value(manifest).map_err(|e| format!("parse:{}", e))
}

#[tauri::command]
pub async fn get_investigator_install_status() -> Result<serde_json::Value, String> {
    require_investigator_entitlement()?;
    let dir = install_dir()?;
    let manifest = verified_installed_manifest();
    let installed = manifest.is_some();
    let version = manifest.map(|manifest| manifest.version);
    serde_json::to_value(InvestigatorInstallStatus {
        installed,
        install_dir: dir.display().to_string(),
        executable_path: dir.join(INVESTIGATOR_BINARY_NAME).display().to_string(),
        version,
    })
    .map_err(|e| format!("parse:{}", e))
}

#[tauri::command]
pub async fn install_investigator_product() -> Result<serde_json::Value, String> {
    require_investigator_entitlement()?;
    let (signed_manifest, manifest) = fetch_manifest().await?;
    let dir = install_dir()?;
    let stage = dir.join(format!(".staging-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&stage).map_err(|e| format!("disk:create staging directory: {}", e))?;
    crate::paths::harden_dir_acl(&stage);

    for artifact in &manifest.artifacts {
        let stage_path = stage.join(&artifact.name);
        if let Err(error) = download_artifact(artifact, &stage_path).await {
            let _ = std::fs::remove_dir_all(&stage);
            return Err(error);
        }
    }
    for artifact in &manifest.artifacts {
        replace_file(&stage.join(&artifact.name), &artifact_path(artifact)?)?;
    }
    let metadata = InstallMetadata {
        schema_version: 1,
        signed_manifest,
    };
    let metadata_tmp = stage.join(INSTALL_METADATA_NAME);
    let encoded =
        serde_json::to_vec(&metadata).map_err(|e| format!("disk:metadata encode: {}", e))?;
    std::fs::write(&metadata_tmp, encoded).map_err(|e| format!("disk:metadata write: {}", e))?;
    replace_file(&metadata_tmp, &metadata_path()?)?;
    let _ = std::fs::remove_dir(&stage);
    crate::log_message_src(
        "info",
        "core",
        &format!(
            "[InvestigatorInstall] installed version {}",
            manifest.version
        ),
    );
    Ok(serde_json::json!({
        "ok": true,
        "version": manifest.version,
        "install_path": dir.join(INVESTIGATOR_BINARY_NAME).display().to_string(),
    }))
}

#[tauri::command]
pub async fn launch_investigator_product() -> Result<(), String> {
    require_investigator_entitlement()?;
    let _manifest = verified_installed_manifest().ok_or_else(|| {
        "validation:Installed Investigator files do not match their signed release. Reinstall required."
            .to_string()
    })?;
    let executable = install_dir()?.join(INVESTIGATOR_BINARY_NAME);
    let mut command = std::process::Command::new(&executable);
    command.current_dir(install_dir()?);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command
        .spawn()
        .map_err(|e| format!("launch:could not start Investigator: {}", e))?;
    Ok(())
}

#[tauri::command]
pub async fn delete_investigator_product() -> Result<(), String> {
    require_investigator_entitlement()?;
    let dir = install_dir()?;
    for name in [
        INVESTIGATOR_BINARY_NAME,
        PRO_SIDECAR_NAME,
        INSTALL_METADATA_NAME,
    ] {
        let path = dir.join(name);
        if path.exists() {
            std::fs::remove_file(&path).map_err(|e| {
                format!(
                    "disk:remove {}: close Investigator first ({})",
                    path.display(),
                    e
                )
            })?;
        }
    }
    Ok(())
}
