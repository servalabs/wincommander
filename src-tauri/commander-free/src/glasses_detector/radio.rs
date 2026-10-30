// SPDX-License-Identifier: AGPL-3.0-or-later
//! WinRT advertisement watcher feeding one bounded scan.

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

pub(super) fn scan(
    seconds: u64,
    near_rssi: i16,
    active: bool,
    include_payload: bool,
) -> ScanSummary {
    let mode = if active { "active" } else { "passive" };
    let tally = Arc::new(Mutex::new(Tally::new(near_rssi, include_payload)));
    let start_failed = |reason: String| {
        Tally::new(near_rssi, include_payload).finish("start_failed", Some(reason), seconds, mode)
    };
    let watcher = match BluetoothLEAdvertisementWatcher::new() {
        Ok(w) => w,
        Err(e) => return start_failed(format!("watcher_create: {e}")),
    };
    let scanning_mode = if active {
        BluetoothLEScanningMode::Active
    } else {
        BluetoothLEScanningMode::Passive
    };
    if let Err(e) = watcher.SetScanningMode(scanning_mode) {
        return start_failed(format!("scanning_mode: {e}"));
    }

    // A fresh salt per scan keeps device counts from linking across scans.
    let salt = std::collections::hash_map::RandomState::new();
    let started = Instant::now();
    let sink = Arc::clone(&tally);
    let received = watcher.Received(&TypedEventHandler::new(
        move |_, args: windows::core::Ref<'_, BluetoothLEAdvertisementReceivedEventArgs>| {
            if let Some(args) = args.as_ref() {
                let device = salt.hash_one(args.BluetoothAddress()?);
                let elapsed_ms = started.elapsed().as_millis() as u64;
                if let Ok(obs) = observation(args, device, elapsed_ms) {
                    if let Ok(mut t) = sink.lock() {
                        t.observe(&obs);
                    }
                }
            }
            Ok(())
        },
    ));
    if let Err(e) = received {
        return start_failed(format!("received_handler: {e}"));
    }

    let we_stopped = Arc::new(AtomicBool::new(false));
    let stopped_early = Arc::new(Mutex::new(None::<String>));
    let (flag, reason) = (Arc::clone(&we_stopped), Arc::clone(&stopped_early));
    let _ = watcher.Stopped(&TypedEventHandler::new(
        move |_, args: windows::core::Ref<'_, BluetoothLEAdvertisementWatcherStoppedEventArgs>| {
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

    if let Err(e) = watcher.Start() {
        return start_failed(format!("start: {e}"));
    }
    let deadline = started + Duration::from_secs(seconds);
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(250));
        let aborted = matches!(
            watcher.Status(),
            Ok(BluetoothLEAdvertisementWatcherStatus::Aborted)
        );
        if aborted || stopped_early.lock().map(|r| r.is_some()).unwrap_or(false) {
            break;
        }
    }
    we_stopped.store(true, Ordering::SeqCst);
    let _ = watcher.Stop();

    let early = stopped_early.lock().ok().and_then(|r| r.clone());
    let (status, reason) = match early {
        Some(r)
            if r == "radio_not_available"
                || r == "disabled_by_policy"
                || r == "disabled_by_user" =>
        {
            ("radio_unavailable", Some(r))
        }
        Some(r) => ("stopped_early", Some(r)),
        None if matches!(
            watcher.Status(),
            Ok(BluetoothLEAdvertisementWatcherStatus::Aborted)
        ) =>
        {
            ("radio_unavailable", Some("aborted".into()))
        }
        None => ("completed", None),
    };
    // The handler may still hold a clone briefly; take the tally by value.
    let tally = std::mem::replace(
        &mut *tally.lock().unwrap_or_else(|p| p.into_inner()),
        Tally::new(near_rssi, include_payload),
    );
    tally.finish(status, reason, seconds, mode)
}
