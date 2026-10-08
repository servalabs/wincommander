// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ActivityEvent {
    id: String,
    device_key: String,
    kind: String,
    at: i64,
    #[serde(default)]
    observed_order: u64,
    verified: bool,
    volume_letter: Option<String>,
}

pub(super) fn merge_events(
    disk: VecDeque<ActivityEvent>,
    memory: &VecDeque<ActivityEvent>,
) -> VecDeque<ActivityEvent> {
    let mut events: BTreeMap<String, ActivityEvent> = disk
        .into_iter()
        .chain(memory.iter().cloned())
        .map(|event| (event.id.clone(), event))
        .collect();
    let mut rows: Vec<_> = std::mem::take(&mut events).into_values().collect();
    rows.sort_by(|a, b| (a.at, a.observed_order, &a.id).cmp(&(b.at, b.observed_order, &b.id)));
    rows.drain(..rows.len().saturating_sub(TIMELINE_CAP));
    rows.into()
}

fn append(
    state: &mut BasicTimeline,
    key: &str,
    kind: &str,
    at: i64,
    verified: bool,
    letter: Option<String>,
) {
    let previous_order = state
        .events
        .back()
        .map(|event| event.observed_order)
        .unwrap_or_default();
    state.events.push_back(ActivityEvent {
        id: uuid::Uuid::new_v4().to_string(),
        device_key: key.to_string(),
        kind: kind.to_string(),
        at,
        observed_order: (SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64)
            .max(previous_order.saturating_add(1)),
        verified,
        volume_letter: letter,
    });
    while state.events.len() > TIMELINE_CAP {
        state.events.pop_front();
    }
}

pub(super) fn observe_mounts(
    state: &mut BasicTimeline,
    current: &BTreeMap<String, BasicIdentity>,
    volumes: &Value,
    now: i64,
    started_at: i64,
) {
    let before = state.observed_mounts.clone();
    let mut after = BTreeSet::new();
    for identity in current.values().filter(|identity| identity.is_mass_storage) {
        for volume in volumes.as_array().into_iter().flatten() {
            if volume
                .get("instanceId")
                .and_then(Value::as_str)
                .is_some_and(|id| id.eq_ignore_ascii_case(&identity.instance_id))
            {
                if let Some(letter) = volume.get("driveLetter").and_then(Value::as_str) {
                    after.insert((identity.key.clone(), letter.to_string()));
                }
            }
        }
    }
    for (key, letter) in before.difference(&after) {
        append(state, key, "unmounted", now, true, Some(letter.clone()));
    }
    for (key, letter) in after.difference(&before) {
        append(state, key, "mounted", now, true, Some(letter.clone()));
    }
    for session in &mut state.sessions {
        if session.attached_at >= started_at && session.detached_at.is_none() {
            let letters: Vec<_> = after
                .iter()
                .filter(|(key, _)| key == &session.device_key)
                .collect();
            session.volume_letter = if letters.len() == 1 {
                Some(letters[0].1.clone())
            } else {
                None
            };
        }
    }
    state.observed_mounts = after;
}

pub(super) async fn control(feature: &str, action: &str, args: Value) -> Result<Value, String> {
    ensure_basic_loaded()?;
    let instance_id = args
        .get("instanceId")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let result = dispatch_paid(feature, "USB device policy", args).await;
    let verified = result
        .as_ref()
        .ok()
        .and_then(|value| value.get("verified"))
        .and_then(Value::as_bool)
        == Some(true);
    let kind = format!(
        "{action}_{}",
        if verified {
            "applied"
        } else if result.is_err() {
            "failed"
        } else {
            "unverified"
        }
    );
    let snapshot = {
        let mut state = basic_state().lock().unwrap();
        if let Some(key) = state
            .records
            .values()
            .find(|record| {
                record
                    .identity
                    .instance_id
                    .eq_ignore_ascii_case(&instance_id)
            })
            .map(|record| record.identity.key.clone())
        {
            append(&mut state, &key, &kind, epoch(), verified, None);
        }
        state.clone()
    };
    let merged = persist_basic_timeline(&snapshot, true).map_err(|error| {
        format!(
            "USB action completed with audit persistence failure; refresh Windows state: {error}"
        )
    })?;
    *basic_state().lock().unwrap() = merged;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mount_changes_follow_identity_even_when_letter_is_reused() {
        let identity = super::super::tests::identity("device-a", "Test drive");
        let current = BTreeMap::from([(identity.key.clone(), identity.clone())]);
        let mut state = BasicTimeline::default();
        let volumes = json!([{"driveLetter":"E:","instanceId":identity.instance_id}]);
        observe_mounts(&mut state, &current, &volumes, 100, 100);
        observe_mounts(&mut state, &current, &volumes, 103, 100);
        assert_eq!(state.events.len(), 1);
        observe_mounts(
            &mut state,
            &current,
            &json!([{"driveLetter":"E:","instanceId":"another-device"}]),
            106,
            100,
        );
        assert_eq!(state.events.len(), 2);
        assert_eq!(state.events[1].kind, "unmounted");
        assert_eq!(state.events[1].device_key, "device-a");
    }
}
