// SPDX-License-Identifier: AGPL-3.0-or-later
//! Short leases prevent unattended backup from outliving mount/policy authority.
use super::*;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, OnceLock,
};
use windows_sys::Win32::Foundation::{CloseHandle, DuplicateHandle, DUPLICATE_SAME_ACCESS, HANDLE};
use windows_sys::Win32::System::Threading::GetCurrentProcess;

struct Lease {
    entry_id: String,
    active: ActiveMount,
    token: usize,
    cancelled: AtomicBool,
    resume: AtomicBool,
}

impl Drop for Lease {
    fn drop(&mut self) {
        // register owns the duplicated token until the last in-flight renewal finishes.
        unsafe {
            CloseHandle(self.token as HANDLE);
        }
    }
}

static LEASES: OnceLock<Mutex<HashMap<String, Arc<Lease>>>> = OnceLock::new();
fn leases() -> &'static Mutex<HashMap<String, Arc<Lease>>> {
    LEASES.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(super) fn key(active: &ActiveMount) -> String {
    format!(
        "{}|{}|{}",
        active.container_identity, active.caller_sid, active.session_id
    )
}

pub(super) fn remove(key: &str) {
    if let Ok(mut records) = leases().lock() {
        if let Some(record) = records.remove(key) {
            record.cancelled.store(true, Ordering::Release);
        }
    }
}

fn retire(record: &Arc<Lease>) {
    if let Ok(mut all) = leases().lock() {
        let id = key(&record.active);
        if all
            .get(&id)
            .is_some_and(|current| Arc::ptr_eq(current, record))
        {
            all.remove(&id);
        }
        record.cancelled.store(true, Ordering::Release);
    }
}

fn request_for(record: &Lease, action: Action) -> Request {
    let mut request = backup_request(&record.active, action);
    request.expected_mount_receipt = Some(sync_management::mount_receipt(
        &record.entry_id,
        &record.active,
    ));
    request
}

pub(super) fn register(
    entry_id: String,
    active: ActiveMount,
    token: HANDLE,
    resume: bool,
) -> Result<(), VaultMountReason> {
    let mut records = leases()
        .lock()
        .map_err(|_| VaultMountReason::BrokerRejected)?;
    let id = key(&active);
    if records.len() >= 128 && !records.contains_key(&id) {
        return Err(VaultMountReason::BrokerRejected);
    }
    let mut owned = std::ptr::null_mut();
    // Duplicate the authenticated pipe token, never reopen a PID or substitute WTS.
    if token.is_null()
        || unsafe {
            DuplicateHandle(
                GetCurrentProcess(),
                token,
                GetCurrentProcess(),
                &mut owned,
                0,
                0,
                DUPLICATE_SAME_ACCESS,
            )
        } == 0
    {
        return Err(VaultMountReason::NotAuthorized);
    }
    let lease = Arc::new(Lease {
        entry_id,
        active,
        token: owned as usize,
        cancelled: AtomicBool::new(false),
        resume: AtomicBool::new(resume),
    });
    if let Some(previous) = records.insert(id, lease) {
        previous.cancelled.store(true, Ordering::Release);
    }
    Ok(())
}

pub(super) fn start(broker: Arc<VaultMountBroker>, store: Arc<VaultAccessStore>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
        loop {
            interval.tick().await;
            let records = leases()
                .lock()
                .map(|records| records.values().cloned().collect::<Vec<_>>())
                .unwrap_or_default();
            let mut tasks = Vec::new();
            for record in records {
                let broker = Arc::clone(&broker);
                let store = Arc::clone(&store);
                // Independent Vaults renew concurrently, so a failed helper cannot
                // starve every other Vault's short authorization lease.
                tasks.push(tokio::spawn(async move {
                    renew_record(&broker, &store, record);
                }));
            }
            for task in tasks {
                let _ = task.await;
            }
        }
    });
}

fn renew_record(broker: &VaultMountBroker, store: &VaultAccessStore, record: Arc<Lease>) {
    let Ok(operation) = operations::for_target(
        &record.active.caller_sid, record.active.session_id, record.active.internal_drive,
    ) else { return; };
    let Ok(_operation) = tokio::task::block_in_place(|| operation.lock()) else { return; };
    // A replacement action holds this same lock through its private call and
    // registration. An old renewal can never dispatch Stop after that launch.
    if record.cancelled.load(Ordering::Acquire) {
        return;
    }
    let authorized = tokio::task::block_in_place(|| {
        let request = request_for(&record, Action::Status);
        broker
            .eligible_cloud_backup_mount(
                store,
                &request,
                record.token as HANDLE,
                record.active.session_id,
                &record.active.caller_sid,
            )
            .is_ok_and(|(id, active)| {
                id == record.entry_id
                    && sync_management::mount_receipt(&id, &active)
                        == sync_management::mount_receipt(&record.entry_id, &record.active)
            })
    });
    if !authorized {
        retire(&record);
        stop_record(record);
        return;
    }
    if record.cancelled.load(Ordering::Acquire) {
        return;
    }
    let feature = if record.resume.swap(false, Ordering::AcqRel) {
        "vault.backup.resume"
    } else {
        "vault.backup.renew"
    };
    let request = request_for(&record, Action::Status);
    let result = backup_call(
        feature,
        next_internal_operation_id(),
        &record.active,
        record.token as HANDLE,
        &request,
    )
    .and_then(|value| {
        serde_json::from_value::<ResultView>(value)
            .map_err(|_| VaultMountReason::BrokerReplyRejected)
    });
    match result {
        Ok(status) if should_renew(&status) => {}
        Ok(status) if status.valid() => retire(&record),
        _ => {
            retire(&record);
            stop_record(record);
        }
    }
}

pub(super) fn should_renew(status: &ResultView) -> bool {
    status.valid()
        && (status.running
            || status.watching
            || status.reason_code.as_deref() == Some("cloud_backup_sign_in_pending"))
}

fn stop_record(record: Arc<Lease>) {
    let request = request_for(&record, Action::Stop);
    let _ = backup_call(
        "vault.backup.stop",
        next_internal_operation_id(),
        &record.active,
        record.token as HANDLE,
        &request,
    );
}

pub(super) fn stop_volume(active: &ActiveMount, wait: bool) {
    let records = match leases().lock() {
        Ok(mut all) => {
            let keys = all
                .iter()
                .filter(|(_, record)| record.active.container_identity == active.container_identity)
                .map(|(key, _)| key.clone())
                .collect::<Vec<_>>();
            keys.into_iter()
                .filter_map(|key| all.remove(&key))
                .inspect(|record| {
                    record.cancelled.store(true, Ordering::Release);
                })
                .collect::<Vec<_>>()
        }
        Err(_) => return,
    };
    if wait {
        let started = std::time::Instant::now();
        for record in records {
            if started.elapsed() >= std::time::Duration::from_secs(10) {
                break;
            }
            stop_record(record);
        }
    } else if let Ok(runtime) = tokio::runtime::Handle::try_current() {
        for record in records {
            runtime.spawn(async move {
                stop_record(record);
            });
        }
    }
}
