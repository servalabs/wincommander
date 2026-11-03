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

fn hold_optional_parent(
    path: &std::path::Path,
) -> Result<Option<Vec<std::fs::File>>, &'static str> {
    match crate::shared_shortcuts_file::hold_parent(path) {
        Ok(guards) => Ok(Some(guards)),
        // A dev-only machine may have no installed shared Start folder at all.
        // Its absence is safe only when there is no link to manage there.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && !entry_exists(path) => {
            Ok(None)
        }
        Err(_) => Err(UNAVAILABLE),
    }
}

fn inactive_without_journal(specs: &[LinkSpec; 2]) -> ClaimReply {
    let clean = specs.iter().all(|spec| {
        !entry_exists(&spec.backup)
            && !entry_exists(&spec.calculator_backup)
            && !entry_exists(&spec.calculator)
    });
    ClaimReply {
        status: if clean { "restored" } else { "partial" },
        warnings: if clean { Vec::new() } else { vec![PARTIAL] },
        managed: Managed {
            desktop: false,
            start: false,
        },
    }
}

fn no_managed_shared_links(journal: &Journal, specs: &[LinkSpec; 2]) -> bool {
    journal.originals.iter().all(Option::is_none)
        && journal.aliases.iter().all(Option::is_none)
        && specs.iter().all(|spec| {
            !entry_exists(&spec.original)
                && !entry_exists(&spec.backup)
                && !entry_exists(&spec.calculator)
                && !entry_exists(&spec.calculator_backup)
        })
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
    let path = root.join("state.json");
    let prior = read_journal(&path)?;
    if prior.is_none() && !active {
        return Ok(inactive_without_journal(&specs));
    }
    let had_journal = prior.is_some();
    let public_guards: Vec<_> = specs
        .iter()
        .map(|spec| hold_optional_parent(&spec.original))
        .collect::<Result<_, _>>()?;
    let mut journal = match prior {
        Some(journal) => journal,
        None => Journal {
            version: 3,
            claims: BTreeMap::new(),
            originals: capture_initial(&specs)?,
            aliases: [None, None],
        },
    };
    #[cfg(debug_assertions)]
    crate::diagnostics::record_shared_shortcut_stage("store_open", trace_started);
    let backup_volume = crate::shared_shortcuts_file::volume_of(guards.last().ok_or(UNAVAILABLE)?)
        .map_err(|_| UNAVAILABLE)?;
    for (original, public_guard) in journal.originals.iter().zip(&public_guards) {
        if let Some(original) = original {
            let public_guard = public_guard.as_ref().ok_or(UNAVAILABLE)?;
            let source_volume =
                crate::shared_shortcuts_file::volume_of(public_guard.last().ok_or(UNAVAILABLE)?)
                    .map_err(|_| UNAVAILABLE)?;
            if source_volume != original.identity.volume || backup_volume != source_volume {
                return Err(UNAVAILABLE);
            }
        }
    }
    journal.set_claim(sid, kind, active);
    // No installed shared links exist on this machine. Do not persist an
    // empty-original journal: a later installer can add links, and the next
    // claim must snapshot those originals before changing them.
    if no_managed_shared_links(&journal, &specs) {
        if had_journal {
            std::fs::remove_file(&path).map_err(|_| UNAVAILABLE)?;
        }
        return Ok(ClaimReply {
            status: if journal.mode() == Mode::Original {
                "restored"
            } else {
                "hidden"
            },
            warnings: Vec::new(),
            managed: Managed {
                desktop: false,
                start: false,
            },
        });
    }
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

    #[test]
    fn absent_shared_folder_without_link_is_unmanaged() {
        let missing = std::env::temp_dir()
            .join(format!(
                "wincommander-missing-{:032x}",
                rand::random::<u128>()
            ))
            .join("WinCommander.lnk");
        assert!(hold_optional_parent(&missing).unwrap().is_none());
    }

    #[test]
    fn inactive_claim_without_journal_or_installed_links_needs_no_shared_folder() {
        let missing = std::env::temp_dir().join(format!(
            "wincommander-shared-{:032x}",
            rand::random::<u128>()
        ));
        let specs = std::array::from_fn(|index| {
            let directory = missing.join(index.to_string());
            LinkSpec {
                original: directory.join("WinCommander.lnk"),
                backup: missing.join(format!("{index}-backup.lnk")),
                target: directory.join("wincommander-free.exe"),
                calculator: directory.join("calculator..lnk"),
                calculator_backup: missing.join(format!("{index}-calculator.lnk")),
                calculator_target: directory.join("Calculator.exe"),
            }
        });
        let reply = inactive_without_journal(&specs);
        assert_eq!(reply.status, "restored");
        assert!(reply.warnings.is_empty());
        assert!(!reply.managed.desktop && !reply.managed.start);
        let mut journal = empty();
        journal.set_claim("S-1-5-21-101", "calculator", true);
        assert!(no_managed_shared_links(&journal, &specs));

        // An installed link appearing later must require a fresh snapshot.
        std::fs::create_dir_all(specs[0].original.parent().unwrap()).unwrap();
        std::fs::write(&specs[0].original, b"installed-link").unwrap();
        assert!(!no_managed_shared_links(&journal, &specs));
        std::fs::remove_file(&specs[0].original).unwrap();
        std::fs::remove_dir_all(&missing).unwrap();
    }
}
