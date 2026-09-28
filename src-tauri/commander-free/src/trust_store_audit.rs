// SPDX-License-Identifier: AGPL-3.0-or-later
//! Read-only inventory of Windows certificate trust stores.
//!
//! This intentionally does not mutate trust. Windows' trusted-root set changes
//! through Windows Update, enterprise policy, and installed software, so the
//! API exposes both the active Root stores and Windows' AuthRoot reference.
//! The UI presents differences as transparency signals, never as malware proof.

use serde::{Deserialize, Serialize};
#[cfg(windows)]
use std::collections::HashSet;
#[cfg(windows)]
use std::process::{Command, Stdio};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrustCertificate {
    pub scope: String,
    pub store: String,
    pub thumbprint: String,
    pub subject: String,
    pub issuer: String,
    pub serial_number: String,
    pub not_before: String,
    pub not_after: String,
    pub signature_algorithm: String,
    pub public_key_algorithm: String,
    pub has_private_key: bool,
    #[serde(default)]
    pub in_windows_auth_root: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrustStoreAudit {
    pub certificates: Vec<TrustCertificate>,
    pub reference_available: bool,
    pub windows_auth_root_count: usize,
}

const INVENTORY_SCRIPT: &str = r#"
$ErrorActionPreference='Stop'
$stores=@(
  [pscustomobject]@{Scope='LocalMachine';Store='Root'},
  [pscustomobject]@{Scope='CurrentUser';Store='Root'},
  [pscustomobject]@{Scope='LocalMachine';Store='CA'},
  [pscustomobject]@{Scope='CurrentUser';Store='CA'},
  [pscustomobject]@{Scope='LocalMachine';Store='TrustedPublisher'},
  [pscustomobject]@{Scope='CurrentUser';Store='TrustedPublisher'},
  [pscustomobject]@{Scope='LocalMachine';Store='AuthRoot'}
)
$items=[System.Collections.Generic.List[object]]::new()
foreach($s in $stores){
  $path="Cert:\$($s.Scope)\$($s.Store)"
  if(-not (Test-Path -LiteralPath $path)){ continue }
  @(Get-ChildItem -LiteralPath $path -ErrorAction Stop) | ForEach-Object {
    $items.Add([pscustomobject]@{
      scope=$s.Scope
      store=$s.Store
      thumbprint=[string]$_.Thumbprint
      subject=[string]$_.Subject
      issuer=[string]$_.Issuer
      serialNumber=[string]$_.SerialNumber
      notBefore=$_.NotBefore.ToUniversalTime().ToString('o')
      notAfter=$_.NotAfter.ToUniversalTime().ToString('o')
      signatureAlgorithm=if($_.SignatureAlgorithm){[string]$_.SignatureAlgorithm.FriendlyName}else{''}
      publicKeyAlgorithm=if($_.PublicKey -and $_.PublicKey.Oid){[string]$_.PublicKey.Oid.FriendlyName}else{''}
      hasPrivateKey=[bool]$_.HasPrivateKey
      inWindowsAuthRoot=$false
    })
  }
}
@($items) | ConvertTo-Json -Compress -Depth 4
"#;

#[tauri::command]
pub async fn trust_store_audit() -> Result<TrustStoreAudit, String> {
    if crate::settings::is_decoy_mode() {
        return Err("Refused: trust-store inventory is unavailable in Decoy mode.".into());
    }

    tokio::task::spawn_blocking(scan_trust_stores)
        .await
        .map_err(|error| format!("trust-store audit task failed: {error}"))?
}

#[cfg(windows)]
fn scan_trust_stores() -> Result<TrustStoreAudit, String> {
    let output = Command::new("powershell.exe")
        .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", INVENTORY_SCRIPT])
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .stdout(Stdio::piped())
        .output()
        .map_err(|error| format!("could not start Windows certificate inventory: {error}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "Windows certificate inventory failed: {}",
            stderr.trim()
        ));
    }

    let stdout = String::from_utf8(output.stdout)
        .map_err(|_| "Windows certificate inventory returned non-UTF-8 output".to_string())?;
    let mut certificates: Vec<TrustCertificate> = serde_json::from_str(stdout.trim())
        .map_err(|error| format!("could not parse Windows certificate inventory: {error}"))?;

    let auth_roots: HashSet<String> = certificates
        .iter()
        .filter(|cert| cert.scope == "LocalMachine" && cert.store == "AuthRoot")
        .map(|cert| cert.thumbprint.to_ascii_uppercase())
        .collect();
    let reference_available = !auth_roots.is_empty();

    for cert in &mut certificates {
        if cert.store == "Root" {
            cert.in_windows_auth_root =
                auth_roots.contains(&cert.thumbprint.to_ascii_uppercase());
        }
    }

    certificates.retain(|cert| cert.store != "AuthRoot");
    certificates.sort_by(|left, right| {
        left.scope
            .cmp(&right.scope)
            .then(left.store.cmp(&right.store))
            .then(left.subject.cmp(&right.subject))
            .then(left.thumbprint.cmp(&right.thumbprint))
    });

    Ok(TrustStoreAudit {
        certificates,
        reference_available,
        windows_auth_root_count: auth_roots.len(),
    })
}

#[cfg(not(windows))]
fn scan_trust_stores() -> Result<TrustStoreAudit, String> {
    Err("Trust-store audit is available only on Windows.".into())
}

#[cfg(test)]
mod tests {
    use super::INVENTORY_SCRIPT;

    #[test]
    fn inventory_script_is_observational_only() {
        for forbidden in [
            "Remove-Item",
            "Remove-ItemProperty",
            "certutil -delstore",
            "Import-Certificate",
            "New-SelfSignedCertificate",
        ] {
            assert!(
                !INVENTORY_SCRIPT.contains(forbidden),
                "trust inventory must stay read-only: {forbidden}"
            );
        }
    }
}
