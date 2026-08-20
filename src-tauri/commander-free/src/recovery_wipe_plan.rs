// SPDX-License-Identifier: AGPL-3.0-or-later
//! Admission checks for a future signed recovery wipe environment.
//!
//! This is NOT an installer or executable recovery environment. It validates a
//! separately signed execution plan plus the existing `wipe_auth` Ed25519
//! device token and stable re-observed disk/GPT facts. A packaged, signed
//! recovery environment must still perform the actual destructive operation.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;
use wincmd_shared::wipe_auth::verify_wipe_token;

pub const WINRE_GPT_TYPE: &str = "DE94BBA4-06D1-4D40-A16A-BFD50179D6AC";
/// Reserved solely for an explicitly owner-approved WinCommander recovery
/// partition.  A signed plan cannot turn an OEM or arbitrary data partition
/// into recovery media by choosing a different GPT type at admission time.
pub const WINCOMMANDER_RECOVERY_GPT_TYPE: &str = "F19D5E12-4B10-4E4D-91C8-2F4F8AE7C301";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiskIdentity<'a> {
    pub disk_number: u32,
    pub serial_or_wwn: &'a str,
    pub device_instance_id: &'a str,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryPartitionIdentity<'a> {
    pub unique_guid: &'a str,
    pub gpt_type: &'a str,
    pub disk: DiskIdentity<'a>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryWipeAdmission<'a> {
    pub device_id: &'a str,
    pub recovery_partition: RecoveryPartitionIdentity<'a>,
    pub approved_target_disks: &'a [DiskIdentity<'a>],
    pub wim_sha256: &'a str,
    pub tools_sha256: &'a str,
    pub one_shot_boot_entry: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryExecutionEvidence<'a> {
    pub device_id: &'a str,
    pub recovery_partition: RecoveryPartitionIdentity<'a>,
    pub target_disks: &'a [DiskIdentity<'a>],
    pub artifacts: &'a VerifiedRecoveryArtifacts,
    pub one_shot_boot_entry: &'a str,
    pub on_external_power: bool,
}

/// Digests produced by reading the staged files. Fields are private so a
/// production caller cannot construct a "verified" artifact set from
/// caller-supplied strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedRecoveryArtifacts {
    wim_sha256: String,
    tools_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedRecoveryPlan<'a> {
    pub version: u8,
    pub admission: &'a RecoveryWipeAdmission<'a>,
    pub wipe_token_nonce: [u8; 32],
    pub expires_at: i64,
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn valid_disk(disk: &DiskIdentity<'_>) -> bool {
    !disk.serial_or_wwn.trim().is_empty()
        && !disk.device_instance_id.trim().is_empty()
        && disk.size_bytes > 0
}

fn same_disk(expected: &DiskIdentity<'_>, observed: &DiskIdentity<'_>) -> bool {
    expected == observed && valid_disk(expected)
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file =
        fs::File::open(path).map_err(|_| "recovery artifact could not be opened".to_string())?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|_| "recovery artifact could not be read".to_string())?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// Hash the exact files staged on the dedicated recovery partition.
pub fn verify_staged_artifacts(
    wim_path: &Path,
    tools_path: &Path,
) -> Result<VerifiedRecoveryArtifacts, String> {
    Ok(VerifiedRecoveryArtifacts {
        wim_sha256: sha256_file(wim_path)?,
        tools_sha256: sha256_file(tools_path)?,
    })
}

fn put_bytes(output: &mut Vec<u8>, value: &[u8]) {
    output.extend_from_slice(&(value.len() as u32).to_be_bytes());
    output.extend_from_slice(value);
}

fn put_str(output: &mut Vec<u8>, value: &str) {
    put_bytes(output, value.as_bytes());
}

fn put_disk(output: &mut Vec<u8>, disk: &DiskIdentity<'_>) {
    output.extend_from_slice(&disk.disk_number.to_be_bytes());
    put_str(output, disk.serial_or_wwn);
    put_str(output, disk.device_instance_id);
    output.extend_from_slice(&disk.size_bytes.to_be_bytes());
}

/// Canonical signed bytes. This is explicit and language-neutral rather than
/// relying on JSON map ordering.
pub fn canonical_signed_plan_bytes(plan: &SignedRecoveryPlan<'_>) -> Vec<u8> {
    let mut output = Vec::new();
    output.extend_from_slice(b"WINCOMMANDER-RECOVERY-WIPE-PLAN\0");
    output.push(plan.version);
    put_str(&mut output, plan.admission.device_id);
    put_str(&mut output, plan.admission.recovery_partition.unique_guid);
    put_str(&mut output, plan.admission.recovery_partition.gpt_type);
    put_disk(&mut output, &plan.admission.recovery_partition.disk);
    output.extend_from_slice(&(plan.admission.approved_target_disks.len() as u32).to_be_bytes());
    for disk in plan.admission.approved_target_disks {
        put_disk(&mut output, disk);
    }
    put_str(&mut output, plan.admission.wim_sha256);
    put_str(&mut output, plan.admission.tools_sha256);
    put_str(&mut output, plan.admission.one_shot_boot_entry);
    output.extend_from_slice(&plan.wipe_token_nonce);
    output.extend_from_slice(&plan.expires_at.to_be_bytes());
    output
}

fn verify_plan_signature(
    plan: &SignedRecoveryPlan<'_>,
    signature_b64: &str,
    verifying_key: &VerifyingKey,
    now_unix: i64,
) -> Result<(), String> {
    if plan.version != 1 || now_unix > plan.expires_at {
        return Err("recovery plan is unsupported or expired".into());
    }
    let bytes = URL_SAFE_NO_PAD
        .decode(signature_b64)
        .map_err(|_| "recovery plan signature is malformed".to_string())?;
    let signature = Signature::from_slice(&bytes)
        .map_err(|_| "recovery plan signature is malformed".to_string())?;
    verifying_key
        .verify(&canonical_signed_plan_bytes(plan), &signature)
        .map_err(|_| "recovery plan signature did not verify".to_string())
}

/// Atomically records a nonce by creating a uniquely named file. A replay sees
/// `AlreadyExists` and fails closed, including across process restarts.
fn consume_nonce_once(store: &Path, nonce: &[u8; 32]) -> Result<(), String> {
    fs::create_dir_all(store).map_err(|_| "recovery nonce store is unavailable".to_string())?;
    let path = store.join(format!("{}.used", hex::encode(nonce)));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                "recovery authorization nonce was already consumed".to_string()
            } else {
                "recovery nonce store is unavailable".to_string()
            }
        })?;
    file.write_all(b"consumed\n")
        .and_then(|_| file.sync_all())
        .map_err(|_| "recovery nonce could not be persisted".to_string())
}

fn valid_recovery_partition(partition: &RecoveryPartitionIdentity<'_>) -> bool {
    !partition.unique_guid.trim().is_empty()
        && partition
            .gpt_type
            .eq_ignore_ascii_case(WINCOMMANDER_RECOVERY_GPT_TYPE)
        && !partition.gpt_type.eq_ignore_ascii_case(WINRE_GPT_TYPE)
        && valid_disk(&partition.disk)
}

/// Validates the arm token and all re-observed immutable target facts. The
/// caller must record the verified nonce durably before erase and clear the
/// one-shot entry on every terminal path. A successful result is admission,
/// never evidence that any firmware sanitize operation completed.
fn validate_admission(
    admission: &RecoveryWipeAdmission<'_>,
    evidence: &RecoveryExecutionEvidence<'_>,
    wipe_token: &str,
    verifying_key: &VerifyingKey,
    now_unix: i64,
) -> Result<(), String> {
    verify_wipe_token(wipe_token, verifying_key, admission.device_id, now_unix)
        .map_err(|error| format!("recovery wipe authorization rejected: {error}"))?;
    if admission.device_id.is_empty()
        || admission.one_shot_boot_entry.is_empty()
        || !is_sha256(admission.wim_sha256)
        || !is_sha256(admission.tools_sha256)
    {
        return Err("recovery admission is incomplete".into());
    }
    if admission.device_id != evidence.device_id
        || admission.one_shot_boot_entry != evidence.one_shot_boot_entry
        || !evidence.on_external_power
    {
        return Err("recovery device, boot entry, or power state changed".into());
    }
    if !valid_recovery_partition(&admission.recovery_partition)
        || !valid_recovery_partition(&evidence.recovery_partition)
        || admission.recovery_partition.unique_guid != evidence.recovery_partition.unique_guid
        || !same_disk(
            &admission.recovery_partition.disk,
            &evidence.recovery_partition.disk,
        )
        || admission.recovery_partition.gpt_type != evidence.recovery_partition.gpt_type
    {
        return Err("recovery partition identity is not approved".into());
    }
    if admission.wim_sha256 != evidence.artifacts.wim_sha256
        || admission.tools_sha256 != evidence.artifacts.tools_sha256
    {
        return Err("recovery artifact hash mismatch".into());
    }
    if admission.approved_target_disks.is_empty()
        || admission.approved_target_disks.len() > 4
        || admission.approved_target_disks.len() != evidence.target_disks.len()
        || !admission
            .approved_target_disks
            .iter()
            .zip(evidence.target_disks)
            .all(|(expected, observed)| same_disk(expected, observed))
    {
        return Err("recovery target disk identity changed since approval".into());
    }
    Ok(())
}

/// Complete pre-execution gate. This validates the device-bound wipe token,
/// verifies the signed plan, hashes the real staged files, rechecks physical
/// disk/partition facts, and consumes the plan nonce. It still does not erase
/// anything; a recovery executor may proceed only after this returns `Ok`.
pub fn validate_and_consume_signed_admission(
    plan: &SignedRecoveryPlan<'_>,
    evidence: &RecoveryExecutionEvidence<'_>,
    wipe_token: &str,
    plan_signature_b64: &str,
    verifying_key: &VerifyingKey,
    now_unix: i64,
    nonce_store: &Path,
) -> Result<(), String> {
    let verified_token = verify_wipe_token(
        wipe_token,
        verifying_key,
        plan.admission.device_id,
        now_unix,
    )
    .map_err(|error| format!("recovery wipe authorization rejected: {error}"))?;
    if verified_token.nonce != plan.wipe_token_nonce {
        return Err("recovery plan is not bound to this wipe authorization".into());
    }
    verify_plan_signature(plan, plan_signature_b64, verifying_key, now_unix)?;
    validate_admission(
        plan.admission,
        evidence,
        wipe_token,
        verifying_key,
        now_unix,
    )?;
    consume_nonce_once(nonce_store, &plan.wipe_token_nonce)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use tempfile::tempdir;
    use wincmd_shared::wipe_auth::{issue_wipe_token, signing_key_from_bytes};

    const HASH: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    const NOW: i64 = 1_750_000_000;
    static TARGET_DISKS: [DiskIdentity<'static>; 1] = [DiskIdentity {
        disk_number: 2,
        serial_or_wwn: "wwn-001",
        device_instance_id: "PCI\\DISK_001",
        size_bytes: 1_000_000,
    }];

    fn disk(number: u32) -> DiskIdentity<'static> {
        DiskIdentity {
            disk_number: number,
            serial_or_wwn: "wwn-001",
            device_instance_id: "PCI\\DISK_001",
            size_bytes: 1_000_000,
        }
    }
    fn partition() -> RecoveryPartitionIdentity<'static> {
        RecoveryPartitionIdentity {
            unique_guid: "{PART-001}",
            gpt_type: WINCOMMANDER_RECOVERY_GPT_TYPE,
            disk: disk(0),
        }
    }
    fn admission() -> RecoveryWipeAdmission<'static> {
        RecoveryWipeAdmission {
            device_id: "device-a",
            recovery_partition: partition(),
            approved_target_disks: &TARGET_DISKS,
            wim_sha256: HASH,
            tools_sha256: HASH,
            one_shot_boot_entry: "{boot-a}",
        }
    }
    fn evidence() -> RecoveryExecutionEvidence<'static> {
        static ARTIFACTS: std::sync::OnceLock<VerifiedRecoveryArtifacts> =
            std::sync::OnceLock::new();
        RecoveryExecutionEvidence {
            device_id: "device-a",
            recovery_partition: partition(),
            target_disks: &TARGET_DISKS,
            artifacts: ARTIFACTS.get_or_init(|| VerifiedRecoveryArtifacts {
                wim_sha256: HASH.into(),
                tools_sha256: HASH.into(),
            }),
            one_shot_boot_entry: "{boot-a}",
            on_external_power: true,
        }
    }
    fn token() -> (String, VerifyingKey) {
        let key = signing_key_from_bytes(&[7; 32]);
        (
            issue_wipe_token("device-a", 300, NOW, &key),
            key.verifying_key(),
        )
    }

    #[test]
    fn requires_a_verified_device_bound_wipe_token() {
        let (token, key) = token();
        assert!(validate_admission(&admission(), &evidence(), &token, &key, NOW).is_ok());
        assert!(validate_admission(&admission(), &evidence(), "bad", &key, NOW).is_err());
    }

    #[test]
    fn rejects_changed_disks_reordered_targets_power_loss_and_winre_partition() {
        let (token, key) = token();
        let admission = admission();
        let mut changed = evidence();
        let changed_targets = [DiskIdentity {
            size_bytes: 2_000_000,
            ..disk(2)
        }];
        changed.target_disks = &changed_targets;
        assert!(validate_admission(&admission, &changed, &token, &key, NOW).is_err());
        let approved = [
            disk(2),
            DiskIdentity {
                disk_number: 3,
                serial_or_wwn: "wwn-002",
                device_instance_id: "PCI\\DISK_002",
                size_bytes: 2_000_000,
            },
        ];
        let reordered = [approved[1].clone(), approved[0].clone()];
        let mut reordered_evidence = evidence();
        reordered_evidence.target_disks = &reordered;
        let two_disk_admission = RecoveryWipeAdmission {
            approved_target_disks: &approved,
            ..admission.clone()
        };
        assert!(
            validate_admission(&two_disk_admission, &reordered_evidence, &token, &key, NOW)
                .is_err()
        );
        let mut power_loss = evidence();
        power_loss.on_external_power = false;
        assert!(validate_admission(&admission, &power_loss, &token, &key, NOW).is_err());
        let mut winre = evidence();
        winre.recovery_partition.gpt_type = WINRE_GPT_TYPE;
        assert!(validate_admission(&admission, &winre, &token, &key, NOW).is_err());
        let mut arbitrary_oem_type = evidence();
        arbitrary_oem_type.recovery_partition.gpt_type = "AABBCCDD-1111-2222-3333-444444444444";
        assert!(validate_admission(&admission, &arbitrary_oem_type, &token, &key, NOW).is_err());
    }

    #[test]
    fn signed_plan_is_bound_to_token_and_nonce_is_consumed_once() {
        let signing: SigningKey = signing_key_from_bytes(&[7; 32]);
        let token = issue_wipe_token("device-a", 300, NOW, &signing);
        let verified =
            verify_wipe_token(&token, &signing.verifying_key(), "device-a", NOW).unwrap();
        let admission = admission();
        let plan = SignedRecoveryPlan {
            version: 1,
            admission: &admission,
            wipe_token_nonce: verified.nonce,
            expires_at: NOW + 300,
        };
        let signature = signing.sign(&canonical_signed_plan_bytes(&plan));
        let signature_b64 = URL_SAFE_NO_PAD.encode(signature.to_bytes());
        let nonce_store = tempdir().unwrap();
        assert!(validate_and_consume_signed_admission(
            &plan,
            &evidence(),
            &token,
            &signature_b64,
            &signing.verifying_key(),
            NOW,
            nonce_store.path(),
        )
        .is_ok());
        assert!(validate_and_consume_signed_admission(
            &plan,
            &evidence(),
            &token,
            &signature_b64,
            &signing.verifying_key(),
            NOW,
            nonce_store.path(),
        )
        .unwrap_err()
        .contains("already consumed"));

        let other = signing_key_from_bytes(&[8; 32]);
        let forged = other.sign(&canonical_signed_plan_bytes(&plan));
        assert!(validate_and_consume_signed_admission(
            &plan,
            &evidence(),
            &token,
            &URL_SAFE_NO_PAD.encode(forged.to_bytes()),
            &signing.verifying_key(),
            NOW,
            tempdir().unwrap().path(),
        )
        .is_err());
    }

    #[test]
    fn staged_artifact_hashes_are_derived_from_file_bytes() {
        let directory = tempdir().unwrap();
        let wim = directory.path().join("recovery.wim");
        let tools = directory.path().join("wipe-tools.bin");
        fs::write(&wim, b"wim-bytes").unwrap();
        fs::write(&tools, b"tool-bytes").unwrap();
        let verified = verify_staged_artifacts(&wim, &tools).unwrap();
        assert_eq!(
            verified.wim_sha256,
            hex::encode(Sha256::digest(b"wim-bytes"))
        );
        assert_eq!(
            verified.tools_sha256,
            hex::encode(Sha256::digest(b"tool-bytes"))
        );
    }
}
