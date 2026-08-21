// SPDX-License-Identifier: AGPL-3.0-or-later
//! Caller-side text truncation to `RuleSetLimits::max_text_bytes`.

/// Truncate `text` to at most `max_bytes`, cutting at the last UTF-8 char
/// boundary at or before that byte offset. Never panics, never splits a
/// multi-byte codepoint, never allocates.
///
/// `CompiledRuleSet::evaluate` takes truncation to `RuleSetLimits::
/// max_text_bytes` as a precondition enforced by the CALLER — it does not
/// re-check the length itself. This is the one correct way to do that
/// truncation: `&text[..max_bytes]` panics whenever `max_bytes` lands mid
/// codepoint, which a raw clipboard read makes routine (multi-byte UTF-8 is
/// common in pasted text).
pub fn truncate_for_match(text: &str, max_bytes: usize) -> &str {
    if text.len() <= max_bytes {
        return text;
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_op_under_the_cap() {
        assert_eq!(truncate_for_match("hello", 100), "hello");
    }

    #[test]
    fn exact_boundary_is_a_no_op() {
        assert_eq!(truncate_for_match("hello", 5), "hello");
    }

    #[test]
    fn cuts_back_to_the_last_char_boundary() {
        // Each 'é' is 2 UTF-8 bytes (U+00E9). A cap of 5 bytes lands mid-
        // codepoint (byte 5 is inside the third 'é'); the result must back
        // off to the boundary at byte 4, keeping exactly two whole chars.
        let text = "ééé"; // 6 bytes, 3 chars
        let truncated = truncate_for_match(text, 5);
        assert_eq!(truncated, "éé");
        assert_eq!(truncated.len(), 4);
    }

    #[test]
    fn zero_cap_yields_empty_string() {
        assert_eq!(truncate_for_match("hello", 0), "");
    }

    #[test]
    fn empty_input_stays_empty() {
        assert_eq!(truncate_for_match("", 10), "");
    }
}
