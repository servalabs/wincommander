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
// Future (planned in ref/roadmap.md Track 1b `shared`):
//
//   - `fs_watcher` — single `notify::RecommendedWatcher` per parent
//     directory, subscriber-fan-out. Replaces three duplicate watchers
//     in `decoy_monitor`, `ransomware_monitor`, `flow_engine`.

pub mod fs_watcher;
pub mod keyboard_hook;
pub mod webhook_server;
