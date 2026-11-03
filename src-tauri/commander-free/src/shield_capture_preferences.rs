use super::AppSettings;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct Preferences {
    enabled: bool,
    capture_on_device: bool,
    capture_on_multi_face: bool,
    revision: String,
}

fn desired(settings: &AppSettings) -> Preferences {
    let shield = &settings.ideal.privacy.privacy_shield;
    Preferences {
        enabled: shield.send_incident_captures_to_fleet,
        capture_on_device: shield.capture_on_device == Some(true),
        capture_on_multi_face: shield.capture_on_multi_face == Some(true),
        revision: String::new(),
    }
}

fn path() -> Result<PathBuf, String> {
    std::env::var_os("LOCALAPPDATA").filter(|value| !value.is_empty())
        .map(PathBuf::from).map(|root| root.join("WinCommander").join("shield_capture_preferences.json"))
        .ok_or_else(|| "Shield capture preferences are unavailable".into())
}

fn read(path: &std::path::Path) -> Result<Option<Preferences>, String> {
    use std::io::Read;
    match std::fs::File::open(path) {
        Ok(file) => {
            let mut bytes = Vec::new();
            file.take(4097).read_to_end(&mut bytes).map_err(|_| "Shield capture preferences could not be read")?;
            if bytes.len() > 4096 { return Err("Shield capture preferences exceed their limit".into()); }
            serde_json::from_slice(&bytes).map(Some).map_err(|_| "Shield capture preferences are invalid".into())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err("Shield capture preferences could not be read".into()),
    }
}

fn persist_at(path: &std::path::Path, mut next: Preferences) -> Result<(), String> {
    if let Ok(Some(previous)) = read(path) {
        next.revision = previous.revision.clone();
        if next == previous && uuid::Uuid::parse_str(&next.revision).is_ok() { return Ok(()); }
    }
    next.revision = uuid::Uuid::new_v4().simple().to_string();
    let parent = path.parent().ok_or("Shield capture preferences have no parent")?;
    std::fs::create_dir_all(parent).map_err(|_| "Shield capture preferences could not be created")?;
    let bytes = serde_json::to_vec(&next).map_err(|_| "Shield capture preferences could not be encoded")?;
    crate::datastore_io::atomic_write(path, &bytes)
        .map_err(|_| "Shield capture preferences could not be saved".into())
}

pub(crate) fn restrict_before_save(settings: &AppSettings) -> Result<(), String> {
    let path = path()?;
    let previous = match read(&path) {
        Ok(Some(previous)) => previous,
        Ok(None) => return Ok(()),
        Err(_) => Preferences::default(),
    };
    let target = desired(settings);
    persist_at(&path, Preferences {
        enabled: previous.enabled && target.enabled,
        capture_on_device: previous.capture_on_device && target.capture_on_device,
        capture_on_multi_face: previous.capture_on_multi_face && target.capture_on_multi_face,
        revision: String::new(),
    })
}

pub(crate) fn publish(settings: &AppSettings) -> Result<(), String> {
    persist_at(&path()?, desired(settings))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preference_revisions_fence_disabled_reenabled_captures() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("preferences.json");
        let enabled = Preferences { enabled: true, capture_on_device: true, ..Default::default() };
        persist_at(&path, enabled.clone()).unwrap();
        let first = read(&path).unwrap().unwrap().revision;
        persist_at(&path, enabled.clone()).unwrap();
        assert_eq!(read(&path).unwrap().unwrap().revision, first);
        persist_at(&path, Preferences::default()).unwrap();
        persist_at(&path, enabled).unwrap();
        assert_ne!(read(&path).unwrap().unwrap().revision, first);
    }
}
