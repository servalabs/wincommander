// SPDX-License-Identifier: AGPL-3.0-or-later
//! Native identity checks. Errors are never evidence that a path is public.
use std::path::{Path, PathBuf};

#[cfg(windows)]
mod driver;
#[cfg(windows)]
mod native;
#[cfg(windows)]
mod operation;
#[cfg(windows)]
pub use operation::VolumeOperationGuard;

/// A snapshot only; re-inspect after acquiring the operation guard and before returning data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VolumeInfo {
    pub root: PathBuf,
    /// VeraCrypt header identity, stable across mount letters and remounts.
    pub stable_id: String,
    /// Stable identity plus the driver's mount-generation counter.
    pub identity: String,
    pub device: String,
    pub is_private: bool,
    pub read_only: bool,
}

/// Inspect an existing local drive path; reject missing paths and reparse aliases.
pub fn inspect_path(path: &Path) -> Result<VolumeInfo, String> {
    #[cfg(windows)]
    {
        native::inspect_path(path)
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Err("Native volume inspection requires Windows".into())
    }
}

/// Enumerate accessible VeraCrypt drive presentations in this caller's namespace.
pub fn mounted_private_volumes() -> Result<Vec<VolumeInfo>, String> {
    #[cfg(windows)]
    {
        native::mounted_private_volumes()
    }
    #[cfg(not(windows))]
    {
        Err("Native volume inspection requires Windows".into())
    }
}

#[cfg(not(windows))]
pub struct VolumeOperationGuard(std::marker::PhantomData<std::rc::Rc<()>>);
#[cfg(not(windows))]
impl VolumeOperationGuard {
    pub fn acquire(_: &VolumeInfo) -> Result<Self, String> {
        Err("Native volume coordination requires Windows".into())
    }
    pub fn acquire_slot(_: u8) -> Result<Self, String> {
        Err("Native volume coordination requires Windows".into())
    }
}

fn private_slot(device: &str) -> Result<u8, String> {
    let lower = device.to_ascii_lowercase();
    let letter = lower
        .strip_prefix(r"\device\veracryptvolume")
        .ok_or("Not a VeraCrypt device")?;
    if letter.len() != 1 || !letter.as_bytes()[0].is_ascii_lowercase() {
        return Err("Invalid VeraCrypt device slot".into());
    }
    Ok(letter.as_bytes()[0] - b'a')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_identity_rejects_prefix_confusion_and_out_of_range_slots() {
        assert_eq!(private_slot(r"\Device\VeraCryptVolumeM"), Ok(12));
        assert_eq!(private_slot(r"\Device\VeraCryptVolumeA"), Ok(0));
        assert_eq!(private_slot(r"\Device\VeraCryptVolumeZ"), Ok(25));
        assert_eq!(private_slot(r"\device\veracryptvolumem"), Ok(12));
        for invalid in [
            r"\Device\VeraCryptVolume12",
            r"\Device\VeraCryptVolume26",
            r"\Device\VeraCryptVolume012",
            r"\Device\VeraCryptVolumeM\child",
            r"\Device\VeraCryptVolumeAA",
            r"\Device\VeraCryptVolume+1",
            r"\Device\HarddiskVolume12",
        ] {
            assert!(private_slot(invalid).is_err());
        }
    }
}
