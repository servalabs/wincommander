// SPDX-License-Identifier: AGPL-3.0-or-later

pub(super) const MAX_MANIFEST_BYTES: usize = 1024 * 1024;
pub(super) const MAX_BINARY_BYTES: usize = 512 * 1024 * 1024;

pub(super) fn checked_body_size(
    current: usize,
    additional: usize,
    limit: usize,
) -> Result<usize, String> {
    current
        .checked_add(additional)
        .filter(|size| *size <= limit)
        .ok_or_else(|| "response exceeds the allowed size".to_string())
}

pub(super) fn check_content_length(length: Option<u64>, limit: usize) -> Result<(), String> {
    if length.is_some_and(|length| length > limit as u64) {
        return Err("advertised response exceeds the allowed size".to_string());
    }
    Ok(())
}

pub(super) fn append_body_chunk(
    body: &mut Vec<u8>,
    chunk: &[u8],
    limit: usize,
) -> Result<(), String> {
    checked_body_size(body.len(), chunk.len(), limit)?;
    body.try_reserve_exact(chunk.len())
        .map_err(|_| "response allocation failed".to_string())?;
    body.extend_from_slice(chunk);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_limit_is_accepted_and_next_byte_is_rejected() {
        assert_eq!(checked_body_size(3, 1, 4), Ok(4));
        assert!(checked_body_size(4, 1, 4).is_err());
        assert!(checked_body_size(usize::MAX, 1, usize::MAX).is_err());
    }

    #[test]
    fn unknown_and_exact_content_lengths_do_not_bypass_the_stream_limit() {
        assert!(check_content_length(None, 4).is_ok());
        assert!(check_content_length(Some(4), 4).is_ok());
        assert!(check_content_length(Some(5), 4).is_err());
        assert!(checked_body_size(3, 2, 4).is_err());
    }

    #[test]
    fn rejected_manifest_chunk_does_not_change_the_accumulated_body() {
        let mut body = vec![b'a'; MAX_MANIFEST_BYTES - 1];
        append_body_chunk(&mut body, b"b", MAX_MANIFEST_BYTES).unwrap();
        assert_eq!(body.len(), MAX_MANIFEST_BYTES);
        assert!(append_body_chunk(&mut body, b"c", MAX_MANIFEST_BYTES).is_err());
        assert_eq!(body.len(), MAX_MANIFEST_BYTES);
        assert_eq!(body.last(), Some(&b'b'));
    }
}
