// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;

pub(super) struct Fake {
    pub(super) admin: bool,
    pub(super) journal: Journal,
    pub(super) adapters: Vec<Adapter>,
    pub(super) external: bool,
    pub(super) fail_write: bool,
    pub(super) fail_write_at: usize,
    pub(super) fail_policy: bool,
    pub(super) fail_disable: bool,
    pub(super) fail_enable: bool,
    pub(super) disables: usize,
    pub(super) enables: usize,
    pub(super) writes: usize,
}

impl Default for Fake {
    fn default() -> Self {
        Self {
            admin: true,
            journal: Journal {
                version: 1,
                ..Default::default()
            },
            adapters: vec![adapter("RADIO1", 0)],
            external: false,
            fail_write: false,
            fail_write_at: 0,
            fail_policy: false,
            fail_disable: false,
            fail_enable: false,
            disables: 0,
            enables: 0,
            writes: 0,
        }
    }
}

pub(super) fn adapter(id: &str, problem: u32) -> Adapter {
    Adapter {
        id: id.into(),
        identity: format!("{id}-hardware"),
        problem,
    }
}

impl Platform for Fake {
    fn admin(&self) -> bool {
        self.admin
    }
    fn read(&mut self) -> Result<Journal, String> {
        Ok(self.journal.clone())
    }
    fn write(&mut self, value: &Journal) -> Result<(), String> {
        self.writes += 1;
        if self.fail_write || self.writes == self.fail_write_at {
            return Err("write failed".into());
        }
        self.journal = value.clone();
        Ok(())
    }
    fn adapters(&mut self) -> Result<Vec<Adapter>, String> {
        Ok(self.adapters.clone())
    }
    fn externally_managed(&mut self) -> Result<bool, String> {
        if self.fail_policy {
            return Err("policy unreadable".into());
        }
        Ok(self.external)
    }
    fn disable(&mut self, target: &Adapter) -> Result<(), String> {
        self.disables += 1;
        assert!(
            self.journal.blocked,
            "disable requires saved machine intent"
        );
        assert!(self.journal.records.iter().any(|r| r.id == target.id));
        if self.fail_disable {
            return Err("disable failed".into());
        }
        self.adapters
            .iter_mut()
            .find(|a| a.id == target.id)
            .unwrap()
            .problem = 22;
        Ok(())
    }
    fn enable(&mut self, target: &Adapter) -> Result<(), String> {
        self.enables += 1;
        assert!(!self.journal.blocked && !self.external);
        assert!(self
            .journal
            .records
            .iter()
            .any(|r| r.id == target.id && r.confirmed));
        if self.fail_enable {
            return Err("enable failed".into());
        }
        self.adapters
            .iter_mut()
            .find(|a| a.id == target.id)
            .unwrap()
            .problem = 0;
        Ok(())
    }
}
