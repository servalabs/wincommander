// SPDX-License-Identifier: AGPL-3.0-or-later
//! Reconcile capture affinity without repeating a compositor mutation.

fn reconcile(
    desired: u32,
    mut read: impl FnMut() -> Result<u32, String>,
    mut write: impl FnMut(u32) -> Result<(), String>,
) -> Result<(), String> {
    // A successful equal read needs no compositor mutation. This keeps the
    // silent-start path from issuing a redundant affinity write.
    if read().is_ok_and(|observed| observed == desired) {
        return Ok(());
    }
    write(desired)?;
    if read()? != desired {
        return Err("Windows did not retain the requested capture-protection state".into());
    }
    Ok(())
}

pub(crate) fn apply(
    hwnd: windows_sys::Win32::Foundation::HWND,
    enabled: bool,
) -> Result<(), String> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowDisplayAffinity, SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE, WDA_NONE,
    };
    let desired = if enabled {
        WDA_EXCLUDEFROMCAPTURE
    } else {
        WDA_NONE
    };
    reconcile(
        desired,
        || {
            let mut observed = 0;
            // The caller supplies only its process-owned main window.
            if unsafe { GetWindowDisplayAffinity(hwnd, &mut observed) } == 0 {
                Err("Windows could not read the window capture-protection state".into())
            } else {
                Ok(observed)
            }
        },
        |affinity| {
            // A successful read-back remains required after a real mutation.
            if unsafe { SetWindowDisplayAffinity(hwnd, affinity) } == 0 {
                Err("SetWindowDisplayAffinity failed (needs Windows 10 2004+)".into())
            } else {
                Ok(())
            }
        },
    )
}

#[cfg(test)]
mod tests {
    use super::reconcile;
    use std::cell::Cell;

    #[test]
    fn unchanged_capture_state_never_mutates_the_compositor() {
        for state in [0, 0x11] {
            let result = reconcile(
                state,
                || Ok(state),
                |_| panic!("A redundant affinity write creates an RDP phantom window"),
            );
            assert!(result.is_ok());
        }
    }

    #[test]
    fn changed_state_is_written_once_and_read_back_in_both_directions() {
        for (before, after) in [(0, 0x11), (0x11, 0)] {
            let current = Cell::new(before);
            let writes = Cell::new(0);
            reconcile(
                after,
                || Ok(current.get()),
                |value| {
                    writes.set(writes.get() + 1);
                    current.set(value);
                    Ok(())
                },
            )
            .unwrap();
            assert_eq!(current.get(), after);
            assert_eq!(writes.get(), 1);
        }
    }

    #[test]
    fn unavailable_initial_observation_still_requires_verified_write() {
        let reads = Cell::new(0);
        assert!(reconcile(
            0x11,
            || {
                reads.set(reads.get() + 1);
                if reads.get() == 1 {
                    Err("unavailable".into())
                } else {
                    Ok(0x11)
                }
            },
            |_| Ok(())
        )
        .is_ok());
        assert_eq!(reads.get(), 2);
    }

    #[test]
    fn failed_changes_never_report_success() {
        assert!(reconcile(0x11, || Ok(0), |_| Err("denied".into())).is_err());
        assert!(reconcile(0x11, || Ok(0), |_| Ok(())).is_err());
        assert!(reconcile(0x11, || Err("unavailable".into()), |_| Ok(())).is_err());
    }
}
