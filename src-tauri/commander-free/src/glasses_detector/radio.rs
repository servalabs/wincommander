// SPDX-License-Identifier: AGPL-3.0-or-later
//! WinRT advertisement watcher shared by the bounded scan and the monitor.

use super::tally::{Observation, Tally};
use super::ScanSummary;
use std::hash::BuildHasher;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use windows::Devices::Bluetooth::Advertisement::{
    BluetoothLEAdvertisementReceivedEventArgs, BluetoothLEAdvertisementWatcher,
    BluetoothLEAdvertisementWatcherStatus, BluetoothLEAdvertisementWatcherStoppedEventArgs,
    BluetoothLEScanningMode,
};
use windows::Devices::Bluetooth::BluetoothError;
use windows::Foundation::TypedEventHandler;
use windows::Storage::Streams::DataReader;

fn guid_text(g: &windows::core::GUID) -> String {
    format!(
        "{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        g.data1,
        g.data2,
        g.data3,
        g.data4[0],
        g.data4[1],
        g.data4[2],
        g.data4[3],
        g.data4[4],
        g.data4[5],
        g.data4[6],
        g.data4[7]
    )
}

fn bluetooth_error_text(e: BluetoothError) -> String {
    match e {
        BluetoothError::RadioNotAvailable => "radio_not_available".into(),
        BluetoothError::ResourceInUse => "resource_in_use".into(),
        BluetoothError::DisabledByPolicy => "disabled_by_policy".into(),
        BluetoothError::DisabledByUser => "disabled_by_user".into(),
        BluetoothError::NotSupported => "not_supported".into(),
        BluetoothError::ConsentRequired => "consent_required".into(),
        other => format!("bluetooth_error_{}", other.0),
    }
}

pub(super) fn is_radio_unavailable(reason: &str) -> bool {
    matches!(
        reason,
        "radio_not_available" | "disabled_by_policy" | "disabled_by_user" | "aborted"
    )
}

fn observation(
    args: &BluetoothLEAdvertisementReceivedEventArgs,
    device: u64,
    elapsed_ms: u64,
) -> windows::core::Result<Observation> {
    let adv = args.Advertisement()?;
    let mut company_ids = Vec::new();
    let mut payloads = Vec::new();
    for md in adv.ManufacturerData()? {
        company_ids.push(md.CompanyId()?);
        let buf = md.Data()?;
        let mut bytes = vec![0u8; buf.Length()? as usize];
        DataReader::FromBuffer(&buf)?.ReadBytes(&mut bytes)?;
        payloads.push(bytes);
    }
    let service_uuids = adv
        .ServiceUuids()?
        .into_iter()
        .map(|g| guid_text(&g))
        .collect();
    let name = adv.LocalName()?.to_string();
    Ok(Observation {
        device,
        rssi: args.RawSignalStrengthInDBm()?,
        elapsed_ms,
        company_ids,
        payloads,
        service_uuids,
        local_name: (!name.is_empty()).then_some(name),
    })
}

/// A running advertisement watcher; stopped on drop.
pub(super) struct Watcher {
    inner: BluetoothLEAdvertisementWatcher,
    we_stopped: Arc<AtomicBool>,
    stop_reason: Arc<Mutex<Option<String>>>,
}

impl Watcher {
    pub(super) fn start(
        active: bool,
        on_observation: impl Fn(Observation) + Send + Sync + 'static,
    ) -> Result<Self, String> {
        let inner =
            BluetoothLEAdvertisementWatcher::new().map_err(|e| format!("watcher_create: {e}"))?;
        let mode = if active {
            BluetoothLEScanningMode::Active
        } else {
            BluetoothLEScanningMode::Passive
        };
        inner
            .SetScanningMode(mode)
            .map_err(|e| format!("scanning_mode: {e}"))?;

        // A fresh salt per watcher keeps device counts from linking across runs.
        let salt = std::collections::hash_map::RandomState::new();
        let started = Instant::now();
        inner
            .Received(
                &TypedEventHandler::new(
                    move |_,
                          args: windows::core::Ref<
                        '_,
                        BluetoothLEAdvertisementReceivedEventArgs,
                    >| {
                        if let Some(args) = args.as_ref() {
                            let device = salt.hash_one(args.BluetoothAddress()?);
                            let elapsed_ms = started.elapsed().as_millis() as u64;
                            if let Ok(obs) = observation(args, device, elapsed_ms) {
                                on_observation(obs);
                            }
                        }
                        Ok(())
                    },
                ),
            )
            .map_err(|e| format!("received_handler: {e}"))?;

        let we_stopped = Arc::new(AtomicBool::new(false));
        let stop_reason = Arc::new(Mutex::new(None::<String>));
        let (flag, reason) = (Arc::clone(&we_stopped), Arc::clone(&stop_reason));
        let _ =
            inner.Stopped(&TypedEventHandler::new(
                move |_,
                      args: windows::core::Ref<
                    '_,
                    BluetoothLEAdvertisementWatcherStoppedEventArgs,
                >| {
                    if !flag.load(Ordering::SeqCst) {
                        let text = args
                            .as_ref()
                            .and_then(|a| a.Error().ok())
                            .map(bluetooth_error_text)
                            .unwrap_or_else(|| "stopped".into());
                        if let Ok(mut r) = reason.lock() {
                            *r = Some(text);
                        }
                    }
                    Ok(())
                },
            ));
        inner.Start().map_err(|e| format!("start: {e}"))?;
        Ok(Self {
            inner,
            we_stopped,
            stop_reason,
        })
    }

    /// Why Windows stopped the watcher on its own, if it did.
    pub(super) fn stop_reason(&self) -> Option<String> {
        if let Some(reason) = self.stop_reason.lock().ok().and_then(|r| r.clone()) {
            return Some(reason);
        }
        matches!(
            self.inner.Status(),
            Ok(BluetoothLEAdvertisementWatcherStatus::Aborted)
        )
        .then(|| "aborted".into())
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        self.we_stopped.store(true, Ordering::SeqCst);
        let _ = self.inner.Stop();
    }
}

pub(super) fn scan(
    seconds: u64,
    near_rssi: i16,
    active: bool,
    include_payload: bool,
) -> ScanSummary {
    let mode = if active { "active" } else { "passive" };
    let tally = Arc::new(Mutex::new(Tally::new(near_rssi, include_payload)));
    let sink = Arc::clone(&tally);
    let watcher = match Watcher::start(active, move |obs| {
        if let Ok(mut t) = sink.lock() {
            t.observe(&obs);
        }
    }) {
        Ok(w) => w,
        Err(reason) => {
            return Tally::new(near_rssi, include_payload).finish(
                "start_failed",
                Some(reason),
                seconds,
                mode,
            )
        }
    };

    let deadline = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < deadline && watcher.stop_reason().is_none() {
        std::thread::sleep(Duration::from_millis(250));
    }
    let (status, reason) = match watcher.stop_reason() {
        Some(r) if is_radio_unavailable(&r) => ("radio_unavailable", Some(r)),
        Some(r) => ("stopped_early", Some(r)),
        None => ("completed", None),
    };
    drop(watcher);
    // The handler may still hold a clone briefly; take the tally by value.
    let tally = std::mem::replace(
        &mut *tally.lock().unwrap_or_else(|p| p.into_inner()),
        Tally::new(near_rssi, include_payload),
    );
    tally.finish(status, reason, seconds, mode)
}
