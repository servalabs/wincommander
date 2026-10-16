// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;
pub(super) struct Fake {
    pub admin: bool,
    pub legacy: bool,
    pub journal: Journal,
    pub observation: Observation,
    pub writes: usize,
    pub applies: usize,
    pub observes: usize,
    pub fail_write_at: usize,
    pub fail_apply: bool,
    pub fail_observe: bool,
    pub after: Option<Observation>,
}
impl Default for Fake {
    fn default() -> Self {
        Self {
            admin: true,
            legacy: false,
            journal: Journal::default(),
            observation: Observation {
                supported: true,
                managed: false,
                context: "a".repeat(64),
                configured: None,
                effective: Some(2),
            },
            writes: 0,
            applies: 0,
            observes: 0,
            fail_write_at: 0,
            fail_apply: false,
            fail_observe: false,
            after: None,
        }
    }
}
impl Platform for Fake {
    fn admin(&self) -> bool {
        self.admin
    }
    fn legacy_active(&mut self) -> Result<bool, String> {
        Ok(self.legacy)
    }
    fn read(&mut self) -> Result<Journal, String> {
        Ok(self.journal.clone())
    }
    fn write(&mut self, journal: &Journal) -> Result<(), String> {
        self.writes += 1;
        if self.writes == self.fail_write_at {
            return Err(STATE_ERROR.into());
        }
        self.journal = journal.clone();
        Ok(())
    }
    fn observe(&mut self) -> Result<Observation, String> {
        self.observes += 1;
        if self.fail_observe {
            return Err(UNVERIFIED.into());
        }
        Ok(self.observation.clone())
    }
    fn apply(&mut self, action: Action) -> Result<Observation, String> {
        self.applies += 1;
        assert!(matches!(
            self.journal.phase,
            Phase::PendingBlock | Phase::PendingAllow
        ));
        if self.fail_apply {
            return Err("[BT_POLICY_REJECTED] Windows rejected the policy.".into());
        }
        match action {
            Action::Block { expected } => {
                assert_eq!(self.observation.configured, expected);
                self.observation.configured = Some(0);
                self.observation.effective = Some(0);
            }
            Action::Allow => {
                assert_eq!(self.observation.configured, Some(0));
                self.observation.configured = Some(2);
                self.observation.effective = Some(2);
            }
        }
        if let Some(after) = self.after.take() {
            self.observation = after;
        }
        Ok(self.observation.clone())
    }
}
