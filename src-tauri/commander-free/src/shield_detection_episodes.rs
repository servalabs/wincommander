// SPDX-License-Identifier: AGPL-3.0-or-later
//! Content-free detector transitions. Only an observed clear closes an interval.

use std::collections::BTreeMap;

pub(super) fn complete_prefix_len(bytes: &[u8]) -> usize {
    bytes
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |index| index + 1)
}

#[derive(Debug, PartialEq)]
pub(super) enum Transition {
    Start {
        class: &'static str,
        episode_id: String,
        started_at_ms: i64,
    },
    Finish {
        class: &'static str,
        episode_id: String,
        duration_seconds: u32,
    },
}

#[derive(Default)]
pub(super) struct Episodes {
    active: BTreeMap<&'static str, (String, i64)>,
    last_timestamp: Option<i64>,
}

impl Episodes {
    /// Stop, rotation and dropped observations cannot prove a detection ended.
    pub(super) fn abandon(&mut self) {
        self.active.clear();
        self.last_timestamp = None;
    }

    #[cfg(test)]
    pub(super) fn observe(
        &mut self,
        classes: &[&'static str],
        timestamp_seconds: Option<f64>,
        mut new_id: impl FnMut() -> String,
    ) -> Vec<Transition> {
        self.observe_with_ids(classes, timestamp_seconds, |_| new_id())
    }

    pub(super) fn observe_with_ids(
        &mut self,
        classes: &[&'static str],
        timestamp_seconds: Option<f64>,
        mut new_id: impl FnMut(&'static str) -> String,
    ) -> Vec<Transition> {
        let Some(timestamp) = timestamp_seconds
            .filter(|t| t.is_finite() && *t > 0.0 && *t < 253_402_300_799.0)
            .map(|t| (t * 1000.0) as i64)
        else {
            self.abandon();
            return Vec::new();
        };
        if self.last_timestamp.is_some_and(|last| timestamp < last) {
            self.abandon();
            return Vec::new();
        }
        self.last_timestamp = Some(timestamp);
        let mut transitions = Vec::new();
        self.active.retain(|class, (episode_id, started)| {
            if classes.contains(class) {
                return true;
            }
            let elapsed = timestamp - *started;
            // Do not clamp a clock jump or an interval beyond the IPC contract.
            if (0..=86_400_000).contains(&elapsed) {
                transitions.push(Transition::Finish {
                    class,
                    episode_id: episode_id.clone(),
                    duration_seconds: (elapsed / 1000) as u32,
                });
            }
            false
        });
        for &class in classes {
            if !matches!(
                class,
                "phone_detected" | "multiple_faces" | "presence_lost" | "look_away"
            ) {
                continue;
            }
            self.active.entry(class).or_insert_with(|| {
                let episode_id = new_id(class);
                transitions.push(Transition::Start {
                    class,
                    episode_id: episode_id.clone(),
                    started_at_ms: timestamp,
                });
                (episode_id, timestamp)
            });
        }
        transitions
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_end_lines_are_retained_until_the_next_read() {
        let start = b"{\"event\":\"look_away\"}\n";
        let partial_end = b"{\"event\":\"look_b";
        let mut bytes = start.to_vec();
        bytes.extend_from_slice(partial_end);
        let offset = complete_prefix_len(&bytes);
        assert_eq!(offset, start.len());
        assert_eq!(complete_prefix_len(&bytes[offset..]), 0);
        bytes.extend_from_slice(b"ack\"}\n");
        assert_eq!(
            &bytes[offset..offset + complete_prefix_len(&bytes[offset..])],
            b"{\"event\":\"look_back\"}\n"
        );
    }

    fn start(class: &'static str, id: &str, millis: i64) -> Transition {
        Transition::Start {
            class,
            episode_id: id.into(),
            started_at_ms: millis,
        }
    }

    fn finish(class: &'static str, id: &str, seconds: u32) -> Transition {
        Transition::Finish {
            class,
            episode_id: id.into(),
            duration_seconds: seconds,
        }
    }

    #[test]
    fn real_clear_uses_detector_time_and_the_original_identity() {
        let mut episodes = Episodes::default();
        assert_eq!(
            episodes.observe(&["phone_detected"], Some(100.25), || "one".into()),
            vec![start("phone_detected", "one", 100250)]
        );
        assert!(episodes
            .observe(&["phone_detected"], Some(102.0), || panic!())
            .is_empty());
        assert_eq!(
            episodes.observe(&[], Some(107.75), || panic!()),
            vec![finish("phone_detected", "one", 7)]
        );
        assert!(episodes.observe(&[], Some(108.0), || panic!()).is_empty());
    }

    #[test]
    fn combined_conditions_end_and_recur_independently() {
        let mut episodes = Episodes::default();
        episodes.observe(&["phone_detected"], Some(100.0), || "phone-1".into());
        assert_eq!(
            episodes.observe(&["phone_detected", "multiple_faces"], Some(103.0), || {
                "faces".into()
            }),
            vec![start("multiple_faces", "faces", 103000)]
        );
        assert_eq!(
            episodes.observe(&["multiple_faces"], Some(105.0), || panic!()),
            vec![finish("phone_detected", "phone-1", 5)]
        );
        assert_eq!(
            episodes.observe(&["multiple_faces", "phone_detected"], Some(106.0), || {
                "phone-2".into()
            }),
            vec![start("phone_detected", "phone-2", 106000)]
        );
        assert_eq!(
            episodes.observe(&[], Some(110.0), || panic!()),
            vec![
                finish("multiple_faces", "faces", 7),
                finish("phone_detected", "phone-2", 4)
            ]
        );
    }

    #[test]
    fn every_supported_kind_gets_its_own_completion() {
        let mut episodes = Episodes::default();
        let classes = [
            "phone_detected",
            "multiple_faces",
            "presence_lost",
            "look_away",
        ];
        let mut serial = 0;
        let starts = episodes.observe(&classes, Some(100.0), || {
            serial += 1;
            serial.to_string()
        });
        let ends = episodes.observe(&[], Some(104.0), || panic!());
        assert_eq!(ends.len(), 4);
        for event in starts {
            let Transition::Start {
                class, episode_id, ..
            } = event
            else {
                panic!()
            };
            assert!(ends.contains(&finish(class, &episode_id, 4)));
        }
    }

    #[test]
    fn stop_rotation_or_dropped_data_leave_duration_unknown() {
        let mut episodes = Episodes::default();
        episodes.observe(&["presence_lost"], Some(100.0), || "old".into());
        episodes.abandon();
        assert!(episodes.observe(&[], Some(200.0), || panic!()).is_empty());
        assert_eq!(
            episodes.observe(&["presence_lost"], Some(201.0), || "new".into()),
            vec![start("presence_lost", "new", 201000)]
        );
    }

    #[test]
    fn invalid_or_regressing_timestamps_cannot_fabricate_a_duration() {
        for invalid in [
            None,
            Some(f64::NAN),
            Some(f64::INFINITY),
            Some(-1.0),
            Some(99.0),
            Some(1e20),
        ] {
            let mut episodes = Episodes::default();
            episodes.observe(&["look_away"], Some(100.0), || "old".into());
            assert!(episodes.observe(&[], invalid, || panic!()).is_empty());
            assert!(episodes.observe(&[], Some(101.0), || panic!()).is_empty());
        }
    }

    #[test]
    fn durations_are_never_clamped_or_rounded_up() {
        for (end, expected) in [(100.5, Some(0)), (86500.0, Some(86400)), (86500.1, None)] {
            let mut episodes = Episodes::default();
            episodes.observe(&["phone_detected"], Some(100.0), || "one".into());
            let events = episodes.observe(&[], Some(end), || panic!());
            assert_eq!(
                events,
                expected
                    .map(|seconds| vec![finish("phone_detected", "one", seconds)])
                    .unwrap_or_default()
            );
        }
    }
}
