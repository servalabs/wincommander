// SPDX-License-Identifier: AGPL-3.0-or-later
//! Recoverable transitions among original, Calculator, and hidden shared links.

use std::io::Write;
use std::path::Path;

use super::{save_journal, Journal, LinkOriginal, LinkSpec, Mode};
use crate::shared_shortcuts_acl;
use crate::shared_shortcuts_file::{
    delete_link, move_link, open_calculator_link, open_link, OpenedLink,
};
use crate::shared_shortcuts_link;

pub(super) struct Outcome {
    pub(super) complete: bool,
    pub(super) managed: [bool; 2],
}

fn same(link: &OpenedLink, identity: &crate::shared_shortcuts_file::LinkIdentity) -> bool {
    &link.identity == identity
}

fn original_private(spec: &LinkSpec, original: &LinkOriginal) -> bool {
    match open_link(&spec.backup, &spec.target) {
        Ok(Some(link)) => {
            same(&link, &original.identity)
                && shared_shortcuts_acl::restrict(&link.file).is_ok()
                && !super::entry_exists(&spec.original)
        }
        Ok(None) => {
            let Ok(Some(link)) = open_link(&spec.original, &spec.target) else {
                return false;
            };
            if !same(&link, &original.identity) {
                return false;
            }
            if shared_shortcuts_acl::restrict(&link.file).is_err() {
                return false;
            }
            if move_link(&link, &spec.backup).is_err() {
                let _ = shared_shortcuts_acl::restore(&link.file, &original.acl);
                return false;
            }
            true
        }
        Err(_) => false,
    }
}

fn original_public(spec: &LinkSpec, original: &LinkOriginal) -> bool {
    match open_link(&spec.original, &spec.target) {
        Ok(Some(link)) => {
            same(&link, &original.identity)
                && shared_shortcuts_acl::restore(&link.file, &original.acl).is_ok()
                && !super::entry_exists(&spec.backup)
        }
        Ok(None) => {
            let Ok(Some(link)) = open_link(&spec.backup, &spec.target) else {
                return false;
            };
            same(&link, &original.identity)
                && move_link(&link, &spec.original).is_ok()
                && shared_shortcuts_acl::restore(&link.file, &original.acl).is_ok()
        }
        Err(_) => false,
    }
}

fn alias_at<'a>(
    path: &Path,
    spec: &LinkSpec,
    expected: &crate::shared_shortcuts_file::LinkIdentity,
) -> Result<Option<OpenedLink>, ()> {
    open_calculator_link(path, &spec.calculator_target)
        .map_err(|_| ())
        .and_then(|found| match found {
            Some(link) if !same(&link, expected) => Err(()),
            other => Ok(other),
        })
}

fn save_alias(journal: &Journal, state: &Path) -> bool {
    save_journal(state, journal).is_ok()
}

fn create_alias(journal: &mut Journal, index: usize, spec: &LinkSpec, state: &Path) -> bool {
    if super::entry_exists(&spec.calculator) {
        return false;
    }
    if !super::entry_exists(&spec.calculator_backup) {
        let Some(bytes) = shared_shortcuts_link::calculator_bytes(&spec.calculator_target) else {
            return false;
        };
        let temporary = spec
            .calculator_backup
            .with_file_name(format!("shortcut-{:032x}.tmp", rand::random::<u128>()));
        let Ok(mut file) = crate::personal_settings::platform::open_record(&temporary, true) else {
            return false;
        };
        if file.write_all(&bytes).is_err() || file.sync_all().is_err() {
            return false;
        }
        drop(file);
        let Ok(Some(link)) = open_calculator_link(&temporary, &spec.calculator_target) else {
            return false;
        };
        if move_link(&link, &spec.calculator_backup).is_err() {
            return false;
        }
    }
    let Ok(Some(link)) = open_calculator_link(&spec.calculator_backup, &spec.calculator_target)
    else {
        return false;
    };
    journal.aliases[index] = Some(link.identity);
    save_alias(journal, state)
}

fn ensure_alias(journal: &mut Journal, index: usize, spec: &LinkSpec, state: &Path) -> bool {
    if journal.aliases[index].is_some() {
        if super::entry_exists(&spec.calculator) || super::entry_exists(&spec.calculator_backup) {
            return true;
        }
        journal.aliases[index] = None;
        if !save_alias(journal, state) {
            return false;
        }
    }
    create_alias(journal, index, spec, state)
}

fn alias_public(
    journal: &mut Journal,
    index: usize,
    spec: &LinkSpec,
    state: &Path,
    original: &LinkOriginal,
) -> bool {
    if journal.aliases[index].is_none() && !ensure_alias(journal, index, spec, state) {
        return false;
    }
    let expected = journal.aliases[index].as_ref().unwrap();
    match alias_at(&spec.calculator, spec, expected) {
        Ok(Some(link)) => {
            shared_shortcuts_acl::restore(&link.file, &original.acl).is_ok()
                && !super::entry_exists(&spec.calculator_backup)
        }
        Ok(None) => {
            let Ok(Some(link)) = alias_at(&spec.calculator_backup, spec, expected) else {
                return false;
            };
            move_link(&link, &spec.calculator).is_ok()
                && shared_shortcuts_acl::restore(&link.file, &original.acl).is_ok()
        }
        Err(_) => false,
    }
}

fn alias_private(journal: &mut Journal, index: usize, spec: &LinkSpec, state: &Path) -> bool {
    let Some(expected) = journal.aliases[index].as_ref() else {
        return !super::entry_exists(&spec.calculator)
            && !super::entry_exists(&spec.calculator_backup);
    };
    match alias_at(&spec.calculator_backup, spec, expected) {
        Ok(Some(link)) => {
            shared_shortcuts_acl::restrict(&link.file).is_ok()
                && !super::entry_exists(&spec.calculator)
        }
        Ok(None) => match alias_at(&spec.calculator, spec, expected) {
            Ok(Some(link)) => {
                if shared_shortcuts_acl::restrict(&link.file).is_err() {
                    return false;
                }
                move_link(&link, &spec.calculator_backup).is_ok()
            }
            Ok(None) => {
                journal.aliases[index] = None;
                save_alias(journal, state)
            }
            Err(_) => false,
        },
        Err(_) => false,
    }
}

fn remove_alias(journal: &mut Journal, index: usize, spec: &LinkSpec, state: &Path) -> bool {
    let Some(expected) = journal.aliases[index].as_ref() else {
        return !super::entry_exists(&spec.calculator)
            && !super::entry_exists(&spec.calculator_backup);
    };
    let private = alias_at(&spec.calculator_backup, spec, expected);
    let public = alias_at(&spec.calculator, spec, expected);
    let link = match (private, public) {
        (Ok(Some(link)), Ok(None)) | (Ok(None), Ok(Some(link))) => link,
        (Ok(None), Ok(None)) => {
            journal.aliases[index] = None;
            return save_alias(journal, state);
        }
        _ => return false,
    };
    if delete_link(&link).is_err() {
        return false;
    }
    drop(link);
    journal.aliases[index] = None;
    save_alias(journal, state)
}

pub(super) fn reconcile(journal: &mut Journal, state: &Path, specs: &[LinkSpec; 2]) -> Outcome {
    let mode = journal.mode();
    let mut result = Outcome {
        complete: true,
        managed: [false, false],
    };
    for (index, spec) in specs.iter().enumerate() {
        let Some(original) = journal.originals[index].clone() else {
            if super::entry_exists(&spec.backup)
                || super::entry_exists(&spec.calculator_backup)
                || super::entry_exists(&spec.calculator)
                || (mode != Mode::Original && super::entry_exists(&spec.original))
            {
                result.complete = false;
            }
            continue;
        };
        let complete = match mode {
            Mode::Original => {
                remove_alias(journal, index, spec, state) && original_public(spec, &original)
            }
            Mode::Hidden => {
                original_private(spec, &original) && alias_private(journal, index, spec, state)
            }
            Mode::Calculator => {
                ensure_alias(journal, index, spec, state)
                    && original_private(spec, &original)
                    && alias_public(journal, index, spec, state, &original)
            }
        };
        let verified = if mode == Mode::Calculator && complete {
            journal.aliases[index].as_ref().is_some_and(|identity| {
                matches!(alias_at(&spec.calculator, spec, identity), Ok(Some(_)))
                    && !super::entry_exists(&spec.original)
                    && !super::entry_exists(&spec.calculator_backup)
            })
        } else {
            false
        };
        result.managed[index] = verified;
        if !complete || (mode == Mode::Calculator && !verified) {
            result.complete = false;
        }
    }
    result
}
