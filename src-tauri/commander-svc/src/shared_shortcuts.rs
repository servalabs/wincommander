// SPDX-License-Identifier: AGPL-3.0-or-later
//! Machine-wide shared links follow authenticated account claims, never renderer paths.

use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

use crate::shared_shortcuts_acl::{self, LinkAcl};
use crate::shared_shortcuts_file::{open_link, LinkIdentity};

const UNAVAILABLE: &str = "shared_shortcuts_unavailable";
const PARTIAL: &str = "A shared WinCommander shortcut needs administrator attention.";
const MAX_JOURNAL_BYTES: u64 = 64 * 1024;

#[derive(Clone, Copy, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Claim {
    calculator: bool,
    hide: bool,
}

impl Claim {
    fn active(self) -> bool {
        self.calculator || self.hide
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    version: u32,
    claims: BTreeMap<String, Claim>,
    originals: [Option<LinkOriginal>; 2],
    aliases: [Option<LinkIdentity>; 2],
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LinkOriginal {
    identity: LinkIdentity,
    acl: LinkAcl,
}

impl Journal {
    fn mode(&self) -> Mode {
        if self.claims.values().any(|claim| claim.hide) {
            Mode::Hidden
        } else if self.claims.values().any(|claim| claim.calculator) {
            Mode::Calculator
        } else {
            Mode::Original
        }
    }

    fn set_claim(&mut self, sid: &str, kind: &str, active: bool) {
        let mut claim = self.claims.get(sid).copied().unwrap_or_default();
        match kind {
            "calculator" => claim.calculator = active,
            "hide" => claim.hide = active,
            _ => unreachable!(),
        }
        if claim.active() {
            self.claims.insert(sid.to_owned(), claim);
        } else {
            self.claims.remove(sid);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Original,
    Calculator,
    Hidden,
}

#[derive(Serialize)]
pub(crate) struct ClaimReply {
    status: &'static str,
    warnings: Vec<&'static str>,
    managed: Managed,
}

#[derive(Serialize)]
struct Managed {
    desktop: bool,
    start: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClaimRequest {
    kind: ClaimKind,
    active: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum ClaimKind {
    Calculator,
    Hide,
}

pub(crate) async fn handle(
    args: serde_json::Value,
    sid: String,
) -> Result<serde_json::Value, &'static str> {
    let request: ClaimRequest = serde_json::from_value(args).map_err(|_| UNAVAILABLE)?;
    let kind = match request.kind {
        ClaimKind::Calculator => "calculator",
        ClaimKind::Hide => "hide",
    };
    let reply = tokio::task::spawn_blocking(move || set_claim(&sid, kind, request.active))
        .await
        .map_err(|_| UNAVAILABLE)??;
    serde_json::to_value(reply).map_err(|_| UNAVAILABLE)
}

#[path = "shared_shortcuts_store.rs"]
mod store;
use store::{clean_old_alias_temps, entry_exists, paths, read_journal, save_journal, LinkSpec};

#[path = "shared_shortcuts_reconcile.rs"]
mod reconciliation;

fn capture_initial(specs: &[LinkSpec; 2]) -> Result<[Option<LinkOriginal>; 2], &'static str> {
    let mut originals = [None, None];
    for (index, spec) in specs.iter().enumerate() {
        if entry_exists(&spec.backup) {
            return Err(UNAVAILABLE);
        }
        originals[index] = open_link(&spec.original, &spec.target)
            .map_err(|_| UNAVAILABLE)?
            .map(|link| {
                shared_shortcuts_acl::snapshot(&link.file).map(|acl| LinkOriginal {
                    identity: link.identity,
                    acl,
                })
            })
            .transpose()
            .map_err(|_| UNAVAILABLE)?;
    }
    Ok(originals)
}

pub(crate) fn set_claim(sid: &str, kind: &str, active: bool) -> Result<ClaimReply, &'static str> {
    #[cfg(debug_assertions)]
    let trace_started = std::time::Instant::now();
    #[cfg(debug_assertions)]
    crate::diagnostics::record_shared_shortcut_stage("store_start", trace_started);
    if sid.is_empty() || !matches!(kind, "calculator" | "hide") {
        return Err(UNAVAILABLE);
    }
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let _lock = LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| UNAVAILABLE)?;
    #[cfg(debug_assertions)]
    crate::diagnostics::record_shared_shortcut_stage("store_lock", trace_started);
    let (root, specs) = paths()?;
    let guards = crate::personal_settings::platform::secure_directory(&root)?;
    clean_old_alias_temps(&root);
    let public_guards: Vec<_> = specs
        .iter()
        .map(|spec| {
            crate::shared_shortcuts_file::hold_parent(&spec.original).map_err(|_| UNAVAILABLE)
        })
        .collect::<Result<_, _>>()?;
    let path = root.join("state.json");
    let mut journal = match read_journal(&path)? {
        Some(journal) => journal,
        None if active => Journal {
            version: 3,
            claims: BTreeMap::new(),
            originals: capture_initial(&specs)?,
            aliases: [None, None],
        },
        None => {
            let clean = specs.iter().all(|spec| {
                !entry_exists(&spec.backup)
                    && !entry_exists(&spec.calculator_backup)
                    && !entry_exists(&spec.calculator)
            });
            return Ok(ClaimReply {
                status: if clean { "restored" } else { "partial" },
                warnings: if clean { Vec::new() } else { vec![PARTIAL] },
                managed: Managed {
                    desktop: false,
                    start: false,
                },
            });
        }
    };
    #[cfg(debug_assertions)]
    crate::diagnostics::record_shared_shortcut_stage("store_open", trace_started);
    let backup_volume = crate::shared_shortcuts_file::volume_of(guards.last().ok_or(UNAVAILABLE)?)
        .map_err(|_| UNAVAILABLE)?;
    for (original, guards) in journal.originals.iter().zip(&public_guards) {
        if let Some(original) = original {
            let source_volume =
                crate::shared_shortcuts_file::volume_of(guards.last().ok_or(UNAVAILABLE)?)
                    .map_err(|_| UNAVAILABLE)?;
            if source_volume != original.identity.volume || backup_volume != source_volume {
                return Err(UNAVAILABLE);
            }
        }
    }
    journal.set_claim(sid, kind, active);
    save_journal(&path, &journal)?;
    #[cfg(debug_assertions)]
    crate::diagnostics::record_shared_shortcut_stage("store_journal", trace_started);
    let outcome = reconciliation::reconcile(&mut journal, &path, &specs);
    #[cfg(debug_assertions)]
    crate::diagnostics::record_shared_shortcut_stage("store_reconcile", trace_started);
    if outcome.complete && journal.mode() == Mode::Original {
        std::fs::remove_file(&path).map_err(|_| UNAVAILABLE)?;
    }
    #[cfg(debug_assertions)]
    crate::diagnostics::record_shared_shortcut_stage("store_end", trace_started);
    Ok(ClaimReply {
        status: if !outcome.complete {
            "partial"
        } else if journal.mode() != Mode::Original {
            "hidden"
        } else {
            "restored"
        },
        warnings: if outcome.complete {
            Vec::new()
        } else {
            vec![PARTIAL]
        },
        managed: Managed {
            desktop: outcome.managed[0],
            start: outcome.managed[1],
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty() -> Journal {
        Journal {
            version: 3,
            claims: BTreeMap::new(),
            originals: [None, None],
            aliases: [None, None],
        }
    }

    #[test]
    fn two_accounts_and_two_modes_do_not_restore_another_accounts_shortcuts() {
        let mut journal = empty();
        journal.set_claim("S-1-5-21-101", "hide", true);
        journal.set_claim("S-1-5-21-202", "calculator", true);
        assert_eq!(journal.mode(), Mode::Hidden);
        journal.set_claim("S-1-5-21-101", "hide", false);
        assert_eq!(journal.mode(), Mode::Calculator);
        assert_eq!(journal.claims.len(), 1);
        journal.set_claim("S-1-5-21-202", "calculator", false);
        assert_eq!(journal.mode(), Mode::Original);
        assert!(journal.claims.is_empty());

        journal.set_claim("S-1-5-21-101", "calculator", true);
        journal.set_claim("S-1-5-21-101", "hide", true);
        journal.set_claim("S-1-5-21-101", "calculator", false);
        assert_eq!(journal.mode(), Mode::Hidden);
        journal.set_claim("S-1-5-21-101", "hide", false);
        assert_eq!(journal.mode(), Mode::Original);
    }

    #[test]
    fn claim_wire_rejects_client_paths_and_unknown_modes() {
        assert!(serde_json::from_value::<ClaimRequest>(serde_json::json!({
            "kind": "hide", "active": true, "path": "C:\\other.lnk"
        }))
        .is_err());
        assert!(serde_json::from_value::<ClaimRequest>(serde_json::json!({
            "kind": "other", "active": true
        }))
        .is_err());
    }
}
