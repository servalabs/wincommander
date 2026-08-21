use super::filters::{is_allowed_shell_verb, is_third_party_verb, safe_disabled_id};
use super::{CachedVerb, ExplorerContextResult, Hive, BACKUP_ROOT, DISABLED_ROOT};

#[cfg(windows)]
pub(super) fn remediate(
    action: &str,
    entries: &[CachedVerb],
) -> Result<ExplorerContextResult, String> {
    use super::windows_registry::{read_string, root};
    let mut backups = Vec::new();
    for entry in entries {
        if !is_allowed_shell_verb(&entry.subkey)
            || !is_third_party_verb(&entry.label, &entry.command)
        {
            return Err("refused an out-of-scope Explorer menu entry".into());
        }
        let current_command_key = if entry.enabled {
            format!("{}\\command", entry.subkey)
        } else {
            let disabled_id = entry
                .disabled_id
                .as_deref()
                .filter(|id| safe_disabled_id(id))
                .ok_or("invalid disabled entry")?;
            format!("{DISABLED_ROOT}\\{disabled_id}\\Entry\\command")
        };
        let current = read_string(
            if entry.enabled {
                root(entry.hive)
            } else {
                root(Hive::CurrentUser)
            },
            &current_command_key,
            "",
        )
        .ok_or("Explorer menu entry changed since scan; scan again")?;
        if current != entry.command || !is_third_party_verb(&entry.label, &current) {
            return Err("Explorer menu entry changed since scan; scan again".into());
        }
        match action {
            "disable" if entry.enabled => backups.push(disable(entry)?),
            "enable" if !entry.enabled => backups.push(enable(entry)?),
            "remove" => backups.push(remove(entry)?),
            _ => return Err("requested action does not match the scanned entry state".into()),
        }
    }
    Ok(ExplorerContextResult {
        changed: entries.len(),
        backup_locations: backups,
    })
}

#[cfg(not(windows))]
pub(super) fn remediate(
    _action: &str,
    _entries: &[CachedVerb],
) -> Result<ExplorerContextResult, String> {
    Err("Explorer menu changes are only available on Windows".into())
}

#[cfg(windows)]
fn disable(entry: &CachedVerb) -> Result<String, String> {
    use super::windows_registry::{backup_and_copy, delete_tree, write_string};
    use uuid::Uuid;
    let disabled_id = Uuid::new_v4().simple().to_string();
    let disabled = format!("{DISABLED_ROOT}\\{disabled_id}");
    let backup = backup_and_copy(entry.hive, &entry.subkey, &format!("{disabled}\\Entry"))?;
    write_string(
        Hive::CurrentUser,
        &disabled,
        "OriginalHive",
        entry.hive.label(),
    )?;
    write_string(
        Hive::CurrentUser,
        &disabled,
        "OriginalSubkey",
        &entry.subkey,
    )?;
    delete_tree(entry.hive, &entry.subkey)?;
    Ok(format!("HKCU\\{backup}"))
}

#[cfg(windows)]
fn enable(entry: &CachedVerb) -> Result<String, String> {
    use super::windows_registry::{backup_and_copy, copy_tree, delete_tree, key_exists};
    let disabled_id = entry
        .disabled_id
        .as_deref()
        .filter(|id| safe_disabled_id(id))
        .ok_or("invalid disabled entry")?;
    let source = format!("{DISABLED_ROOT}\\{disabled_id}\\Entry");
    if key_exists(entry.hive, &entry.subkey) {
        return Err("refused to overwrite an existing Explorer menu entry".into());
    }
    let backup = backup_and_copy(
        Hive::CurrentUser,
        &source,
        &format!("{BACKUP_ROOT}\\{}", entry.id),
    )?;
    copy_tree(Hive::CurrentUser, &source, entry.hive, &entry.subkey)?;
    delete_tree(
        Hive::CurrentUser,
        &format!("{DISABLED_ROOT}\\{disabled_id}"),
    )?;
    Ok(format!("HKCU\\{backup}"))
}

#[cfg(windows)]
fn remove(entry: &CachedVerb) -> Result<String, String> {
    use super::windows_registry::{backup_and_copy, delete_tree};
    if entry.enabled {
        let backup = backup_and_copy(
            entry.hive,
            &entry.subkey,
            &format!("{BACKUP_ROOT}\\{}", entry.id),
        )?;
        delete_tree(entry.hive, &entry.subkey)?;
        Ok(format!("HKCU\\{backup}"))
    } else {
        let id = entry
            .disabled_id
            .as_deref()
            .filter(|id| safe_disabled_id(id))
            .ok_or("invalid disabled entry")?;
        let source = format!("{DISABLED_ROOT}\\{id}");
        let backup = backup_and_copy(
            Hive::CurrentUser,
            &source,
            &format!("{BACKUP_ROOT}\\{}", entry.id),
        )?;
        delete_tree(Hive::CurrentUser, &source)?;
        Ok(format!("HKCU\\{backup}"))
    }
}
