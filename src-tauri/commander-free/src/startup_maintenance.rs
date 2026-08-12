// SPDX-License-Identifier: AGPL-3.0-or-later
//! Read-only startup impact metadata. Existing startup-manager toggles own mutations.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

const ENTRY_LIMIT: usize = 128;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupImpactScan {
    pub entries: Vec<StartupImpactEntry>,
    pub truncated: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupImpactEntry {
    pub id: String,
    pub name: String,
    pub source: String,
    pub location: String,
    pub command: String,
    pub executable_path: Option<String>,
    pub path_exists: bool,
    pub signature_status: String,
    pub signer: Option<String>,
    pub impact: String,
    pub recommendation: String,
}

struct PendingEntry {
    name: String,
    source: String,
    location: String,
    command: String,
}

#[tauri::command]
pub async fn startup_impact_scan() -> Result<StartupImpactScan, String> {
    tokio::task::spawn_blocking(scan_startup_entries)
        .await
        .map_err(|error| format!("startup impact scan task failed: {error}"))?
}

fn scan_startup_entries() -> Result<StartupImpactScan, String> {
    let mut entries = registry_entries();
    entries.extend(startup_folder_entries());
    entries.sort_by(|a, b| {
        a.location
            .cmp(&b.location)
            .then_with(|| a.name.cmp(&b.name))
    });
    let truncated = entries.len() > ENTRY_LIMIT;
    entries.truncate(ENTRY_LIMIT);

    let paths: Vec<String> = entries
        .iter()
        .filter_map(|entry| executable_path(&entry.command))
        .filter(|path| path.is_file())
        .map(|path| path.to_string_lossy().into_owned())
        .collect();
    let signatures = read_signatures(&paths)?;
    let entries = entries
        .into_iter()
        .map(|entry| enrich_entry(entry, &signatures))
        .collect();
    Ok(StartupImpactScan { entries, truncated })
}

fn registry_entries() -> Vec<PendingEntry> {
    crate::runtime_visibility::registry::read_run_values()
        .into_iter()
        .take(ENTRY_LIMIT + 1)
        .map(|entry| PendingEntry {
            name: entry.name,
            source: "registry".into(),
            location: format!("{}\\{}", entry.hive, entry.subkey),
            command: entry.command,
        })
        .collect()
}

fn startup_folder_entries() -> Vec<PendingEntry> {
    let mut entries = Vec::new();
    for (name, path) in [
        ("userStartup", std::env::var_os("APPDATA")),
        ("commonStartup", std::env::var_os("ProgramData")),
    ] {
        let Some(root) = path else { continue };
        let folder = PathBuf::from(root).join("Microsoft\\Windows\\Start Menu\\Programs\\Startup");
        let Ok(read_dir) = std::fs::read_dir(&folder) else {
            continue;
        };
        for file in read_dir
            .flatten()
            .take(ENTRY_LIMIT + 1)
            .filter(|file| file.file_type().is_ok_and(|kind| kind.is_file()))
        {
            let path = file.path();
            entries.push(PendingEntry {
                name: path
                    .file_stem()
                    .and_then(|value| value.to_str())
                    .unwrap_or_default()
                    .into(),
                source: name.into(),
                location: path.to_string_lossy().into_owned(),
                command: path.to_string_lossy().into_owned(),
            });
        }
    }
    entries
}

fn enrich_entry(
    entry: PendingEntry,
    signatures: &HashMap<String, Signature>,
) -> StartupImpactEntry {
    let executable_path = executable_path(&entry.command);
    let path_exists = executable_path.as_ref().is_some_and(|path| path.is_file());
    let signature = executable_path
        .as_ref()
        .and_then(|path| signatures.get(&path.to_string_lossy().to_string()));
    let (impact, recommendation) = classify(&entry.name, &entry.command, path_exists);
    StartupImpactEntry {
        id: format!("{}:{}", entry.location, entry.name),
        name: entry.name,
        source: entry.source,
        location: entry.location,
        command: entry.command,
        executable_path: executable_path.map(|path| path.to_string_lossy().into_owned()),
        path_exists,
        signature_status: signature
            .map(|value| value.status.clone())
            .unwrap_or_else(|| {
                if path_exists {
                    "unknown".into()
                } else {
                    "missing".into()
                }
            }),
        signer: signature.and_then(|value| value.signer.clone()),
        impact,
        recommendation,
    }
}

fn classify(name: &str, command: &str, path_exists: bool) -> (String, String) {
    let text = format!("{name} {command}").to_ascii_lowercase();
    if !path_exists {
        return ("unknown".into(), "review".into());
    }
    if [
        "securityhealth",
        "windowsdefender",
        "synaptics",
        "elan",
        "realtek",
        "nvidia",
        "intel",
        "amd",
    ]
    .iter()
    .any(|hint| text.contains(hint))
    {
        return ("unknown".into(), "keep".into());
    }
    ("unknown".into(), "review".into())
}

fn executable_path(command: &str) -> Option<PathBuf> {
    let token = command
        .trim()
        .strip_prefix('"')
        .map(|value| value.split('"').next().unwrap_or_default())
        .unwrap_or_else(|| command.split_whitespace().next().unwrap_or_default());
    let expanded = expand_environment(token);
    let path = PathBuf::from(expanded);
    path.is_absolute().then_some(path)
}

fn expand_environment(value: &str) -> String {
    let mut expanded = value.to_string();
    for variable in [
        "APPDATA",
        "LOCALAPPDATA",
        "ProgramData",
        "ProgramFiles",
        "ProgramFiles(x86)",
        "SystemRoot",
        "WINDIR",
    ] {
        if let Some(replacement) = std::env::var_os(variable) {
            expanded = expanded.replace(&format!("%{variable}%"), &replacement.to_string_lossy());
        }
    }
    expanded
}

#[derive(Deserialize)]
struct Signature {
    path: String,
    status: String,
    signer: Option<String>,
}

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[cfg(windows)]
fn read_signatures(paths: &[String]) -> Result<HashMap<String, Signature>, String> {
    use std::os::windows::process::CommandExt;

    if paths.is_empty() {
        return Ok(HashMap::new());
    }
    const SCRIPT: &str = "$ErrorActionPreference='Stop';$p=@($env:WINCOMMANDER_STARTUP_SIGN_PATHS|ConvertFrom-Json);@($p|ForEach-Object{$s=Get-AuthenticodeSignature -LiteralPath $_;[pscustomobject]@{path=$_;status=$s.Status.ToString();signer=if($s.SignerCertificate){$s.SignerCertificate.Subject}else{$null}}})|ConvertTo-Json -Compress";
    let output = match std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", SCRIPT])
        .env(
            "WINCOMMANDER_STARTUP_SIGN_PATHS",
            serde_json::to_string(paths).map_err(|error| error.to_string())?,
        )
        // KT: PowerShell is an implementation detail; never flash a console
        // window while the desktop app performs a read-only signature check.
        .creation_flags(CREATE_NO_WINDOW)
        .output()
    {
        Ok(output) => output,
        Err(_) => return Ok(HashMap::new()),
    };
    if !output.status.success() {
        return Ok(HashMap::new());
    }
    let records = parse_signature_records(&output.stdout).unwrap_or_default();
    Ok(records
        .into_iter()
        .map(|record| (record.path.clone(), record))
        .collect())
}

#[cfg(not(windows))]
fn read_signatures(_paths: &[String]) -> Result<HashMap<String, Signature>, String> {
    Ok(HashMap::new())
}

fn parse_signature_records(bytes: &[u8]) -> Result<Vec<Signature>, serde_json::Error> {
    let value: serde_json::Value = serde_json::from_slice(bytes)?;
    match value {
        serde_json::Value::Array(_) => serde_json::from_value(value),
        value => serde_json::from_value(value).map(|record| vec![record]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_quoted_executable_paths() {
        assert_eq!(
            executable_path(r#""C:\Program Files\App\app.exe" --silent"#),
            Some(PathBuf::from(r"C:\Program Files\App\app.exe"))
        );
    }
    #[test]
    fn missing_entries_never_get_a_disable_recommendation() {
        assert_eq!(classify("unknown", "C:\\gone.exe", false).1, "review");
    }
    #[test]
    fn accepts_a_single_signature_response() {
        let rows =
            parse_signature_records(br#"{"path":"C:\\app.exe","status":"Valid","signer":null}"#)
                .unwrap();
        assert_eq!(rows[0].status, "Valid");
    }
}
