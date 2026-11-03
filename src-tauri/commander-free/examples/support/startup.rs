// SPDX-License-Identifier: AGPL-3.0-or-later
//! Minimal calculator state for isolated window probes.
use std::sync::Mutex;

pub struct CalcModeState(pub Mutex<CalcModeSnapshot>);

#[derive(Default)]
pub struct CalcModeSnapshot {
    pub active: bool,
    pub generation: u32,
}

impl CalcModeState {
    pub fn new() -> Self {
        Self(Mutex::new(CalcModeSnapshot::default()))
    }
}

pub mod startup_auth {
    pub fn enter_calculator_mode_with(_: tauri::WebviewWindow, _: bool) -> Result<(), String> {
        Err("Calculator mode is unavailable in this isolated window probe".into())
    }
}
