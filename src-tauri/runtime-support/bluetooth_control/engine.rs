// SPDX-License-Identifier: AGPL-3.0-or-later
use serde::{Deserialize, Serialize};

pub(super) const UNVERIFIED: &str = "[BT_POLICY_UNVERIFIED] Windows Bluetooth policy could not be verified. Refresh or contact your administrator.";
pub(super) const PENDING: &str = "[BT_POLICY_PENDING] A Bluetooth policy change is unconfirmed. Review its state with your administrator before another change.";
pub(super) const MANAGED: &str = "[BT_POLICY_MANAGED] Another policy or changed management context controls Bluetooth. Ask your administrator to review it.";
pub(super) const UNSUPPORTED: &str = "[BT_POLICY_UNSUPPORTED] This Windows edition does not have a supported Bluetooth policy provider. No device settings were changed.";
pub(super) const LEGACY: &str = "[BT_LEGACY_RECOVERY] An earlier adapter restriction needs administrator review. The policy-only control will not change adapters or discard its recovery record.";
pub(super) const STATE_ERROR: &str = "[BT_STATE_UNVERIFIED] Bluetooth policy ownership could not be verified. No policy will be changed.";

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BluetoothStatus {
    pub schema_version: u32,
    pub blocked: bool,
    pub state: String,
    pub can_change: bool,
    pub message: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Phase {
    #[default]
    Idle,
    PendingBlock,
    Confirmed,
    PendingAllow,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Journal {
    pub version: u32,
    pub blocked: bool,
    pub phase: Phase,
    pub baseline: Option<i32>,
    pub context: Option<String>,
}
impl Default for Journal {
    fn default() -> Self {
        Self {
            version: 2,
            blocked: false,
            phase: Phase::Idle,
            baseline: None,
            context: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Observation {
    pub supported: bool,
    pub managed: bool,
    pub context: String,
    pub configured: Option<i32>,
    pub effective: Option<i32>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Action {
    Block { expected: Option<i32> },
    Allow,
}
pub(super) trait Platform {
    fn admin(&self) -> bool;
    fn legacy_active(&mut self) -> Result<bool, String>;
    fn read(&mut self) -> Result<Journal, String>;
    fn write(&mut self, journal: &Journal) -> Result<(), String>;
    fn observe(&mut self) -> Result<Observation, String>;
    fn apply(&mut self, action: Action) -> Result<Observation, String>;
}

fn read(platform: &mut impl Platform) -> Result<Journal, String> {
    let value = platform.read()?;
    let valid_context = value.context.as_ref().is_some_and(|context| {
        context.len() == 64 && context.bytes().all(|byte| byte.is_ascii_hexdigit())
    });
    if value.version != 2
        || !matches!(value.baseline, None | Some(2))
        || (value.phase == Phase::Idle
            && (value.blocked || value.context.is_some() || value.baseline.is_some()))
        || (value.phase != Phase::Idle && !valid_context)
        || (matches!(value.phase, Phase::Confirmed | Phase::PendingBlock) && !value.blocked)
        || (value.phase == Phase::PendingAllow && value.blocked)
    {
        return Err(STATE_ERROR.into());
    }
    Ok(value)
}

fn checked_observe(platform: &mut impl Platform) -> Result<Observation, String> {
    let value = platform.observe()?;
    validate_observation(&value)?;
    Ok(value)
}
fn validate_observation(value: &Observation) -> Result<(), String> {
    if value.supported
        && (!matches!(value.configured, None | Some(0 | 2))
            || !matches!(value.effective, Some(0 | 2))
            || value.context.len() != 64
            || !value.context.bytes().all(|byte| byte.is_ascii_hexdigit()))
    {
        return Err(UNVERIFIED.into());
    }
    Ok(())
}

fn reply(
    platform: &impl Platform,
    journal: &Journal,
    observation: &Observation,
) -> BluetoothStatus {
    let same_context = journal
        .context
        .as_ref()
        .map_or(true, |context| context == &observation.context);
    let (state, message) = if !observation.supported {
        ("unsupported", UNSUPPORTED)
    } else if observation.managed || !same_context {
        ("externally_managed", MANAGED)
    } else if matches!(journal.phase, Phase::PendingBlock | Phase::PendingAllow) {
        ("pending", PENDING)
    } else if journal.phase == Phase::Confirmed
        && observation.configured == Some(0)
        && observation.effective == Some(0)
    {
        (
            "blocked",
            "Bluetooth is blocked by WinCommander's machine policy. Paired devices are preserved.",
        )
    } else if journal.phase != Phase::Idle
        || observation.configured == Some(0)
        || observation.effective == Some(0)
    {
        ("externally_managed", MANAGED)
    } else {
        ("allowed", "WinCommander's Bluetooth policy is released. Windows radio settings and other policies still apply.")
    };
    BluetoothStatus {
        schema_version: 2,
        blocked: journal.blocked,
        state: state.into(),
        can_change: platform.admin() && matches!(state, "allowed" | "blocked"),
        message: message.into(),
    }
}

fn legacy_reply() -> BluetoothStatus {
    BluetoothStatus {
        schema_version: 2,
        blocked: false,
        state: "legacy_recovery_required".into(),
        can_change: false,
        message: LEGACY.into(),
    }
}

pub(super) fn status(platform: &mut impl Platform) -> Result<BluetoothStatus, String> {
    if platform.legacy_active()? {
        return Ok(legacy_reply());
    }
    let journal = read(platform)?;
    let observation = checked_observe(platform)?;
    Ok(reply(platform, &journal, &observation))
}

pub(super) fn set(platform: &mut impl Platform, blocked: bool) -> Result<BluetoothStatus, String> {
    if !platform.admin() {
        return Err("[BT_ACCESS_DENIED] Administrator approval is required to change Bluetooth policy for this PC.".into());
    }
    if platform.legacy_active()? {
        return Err(LEGACY.into());
    }
    let mut journal = read(platform)?;
    let before = checked_observe(platform)?;
    if !before.supported {
        return Err(UNSUPPORTED.into());
    }
    if before.managed
        || journal
            .context
            .as_ref()
            .is_some_and(|context| context != &before.context)
    {
        return Err(MANAGED.into());
    }
    if matches!(journal.phase, Phase::PendingBlock | Phase::PendingAllow) {
        return Err(PENDING.into());
    }
    if blocked && journal.phase == Phase::Confirmed {
        if before.configured == Some(0) && before.effective == Some(0) {
            return Ok(reply(platform, &journal, &before));
        }
        return Err(MANAGED.into());
    }
    if !blocked && journal.phase == Phase::Idle {
        if before.configured == Some(0) || before.effective == Some(0) {
            return Err(MANAGED.into());
        }
        return Ok(reply(platform, &journal, &before));
    }
    let action = if blocked {
        if before.configured == Some(0) || before.effective != Some(2) {
            return Err(MANAGED.into());
        }
        journal.baseline = before.configured;
        journal.context = Some(before.context.clone());
        journal.phase = Phase::PendingBlock;
        Action::Block {
            expected: before.configured,
        }
    } else {
        if journal.phase != Phase::Confirmed
            || before.configured != Some(0)
            || before.effective != Some(0)
        {
            return Err(MANAGED.into());
        }
        journal.phase = Phase::PendingAllow;
        Action::Allow
    };
    journal.blocked = blocked;
    platform.write(&journal)?;
    let after = platform.apply(action)?;
    validate_observation(&after)?;
    if !after.supported || after.managed || before.context != after.context {
        return Err(UNVERIFIED.into());
    }
    if blocked {
        if after.configured != Some(0) || after.effective != Some(0) {
            return Err(UNVERIFIED.into());
        }
        journal.phase = Phase::Confirmed;
    } else {
        // 2 is the CSP's documented neutral allow default. We deliberately do
        // not delete the shared Connectivity instance to restore leaf absence.
        if after.configured != Some(2) {
            return Err(UNVERIFIED.into());
        }
        journal = Journal::default();
    }
    platform.write(&journal)?;
    Ok(reply(platform, &journal, &after))
}

#[cfg(test)]
#[path = "failure_tests.rs"]
mod failure_tests;
#[cfg(test)]
#[path = "test_support.rs"]
mod test_support;
#[cfg(test)]
#[path = "tests.rs"]
mod tests;
