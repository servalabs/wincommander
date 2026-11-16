// src-tauri/commander-free/src/services/mod.rs
//
// ═══════════════════════════════════════════════════════════════════════
// SHARED SERVICES — utilities reused by 3+ feature modules
// ═══════════════════════════════════════════════════════════════════════
//
// Modules in this folder are *infrastructure*, not features. They expose
// ref-counted, subscriber-style APIs so feature modules can plug in
// without duplicating Win32 / `notify` / cron / etc. plumbing.
//
// Current modules:
//
//   - `keyboard_hook` — single WH_KEYBOARD_LL hook shared by
//     coercion-phrase (phrase matching) and the Flows engine
//     (KeySequenceTrigger). Replaces the per-module duplicate that lived
//     in `coercion_phrase.rs` and the frontend-bridge stopgap in
//     `flow_engine.rs::listen_key_sequence`.
//
//   - `fs_watcher` — single `notify::RecommendedWatcher` per parent
//     directory, subscriber fan-out, shared by `ransomware_monitor`,
//     `flow_engine` and `downloads_watcher`.
//   - `webhook_server` — shared local webhook listener.

pub mod fs_watcher;
pub mod keyboard_hook;
pub mod webhook_server;
