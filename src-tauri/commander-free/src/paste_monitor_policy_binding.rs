// SPDX-License-Identifier: AGPL-3.0-or-later
use clipboard_guard_helper::policy::ClipboardPolicyResponse;

pub(super) fn accepts_fetched_policy(
    requested: Option<&str>,
    current: Option<&str>,
    response: &ClipboardPolicyResponse,
    active_version: i64,
) -> bool {
    if requested != current
        || response.reporting_binding.as_deref() != current
        || response.policy_version < active_version
    {
        return false;
    }
    current.is_some()
        || (response.policy_version == 0
            && response.rules.is_empty()
            && response.content_rules.is_none())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn policy(binding: Option<&str>, version: i64) -> ClipboardPolicyResponse {
        ClipboardPolicyResponse {
            policy_version: version,
            rules: vec![],
            content_rules: None,
            reporting_binding: binding.map(str::to_owned),
        }
    }
    #[test]
    fn an_old_async_fetch_cannot_be_associated_with_a_new_enrollment() {
        assert!(!accepts_fetched_policy(
            Some("A"),
            Some("B"),
            &policy(Some("A"), 10),
            0
        ));
        assert!(!accepts_fetched_policy(
            Some("B"),
            Some("B"),
            &policy(Some("A"), 10),
            0
        ));
        assert!(!accepts_fetched_policy(
            None,
            Some("B"),
            &policy(Some("B"), 10),
            0
        ));
        assert!(accepts_fetched_policy(
            Some("B"),
            Some("B"),
            &policy(Some("B"), 1),
            0
        ));
    }
    #[test]
    fn slower_fetches_cannot_replace_a_newer_policy_in_the_same_enrollment() {
        assert!(!accepts_fetched_policy(
            Some("A"),
            Some("A"),
            &policy(Some("A"), 9),
            10
        ));
        assert!(accepts_fetched_policy(
            Some("A"),
            Some("A"),
            &policy(Some("A"), 10),
            10
        ));
        assert!(accepts_fetched_policy(None, None, &policy(None, 0), 0));
        assert!(!accepts_fetched_policy(None, None, &policy(None, 1), 0));
    }
}
