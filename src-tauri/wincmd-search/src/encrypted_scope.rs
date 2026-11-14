// SPDX-License-Identifier: AGPL-3.0-or-later
//! Context-bound envelope for user-protected experimental search scope.
use std::io;
use zeroize::Zeroizing;
const MAGIC: &[u8; 8] = b"WCSCOPE2";
pub(crate) const MAX_SCOPE: usize = 60 * 1024;
#[cfg(all(windows, feature = "experimental-encrypted-search"))]
pub(crate) const MAX_PROTECTED_SCOPE: usize = 65_536;

pub(crate) fn envelope(scope: &[u8]) -> io::Result<Zeroizing<Vec<u8>>> {
    if scope.len() > MAX_SCOPE { return Err(io::Error::other("scope exceeds limit")); }
    let mut value = Zeroizing::new(Vec::with_capacity(MAGIC.len() + scope.len()));
    value.extend_from_slice(MAGIC); value.extend_from_slice(scope); Ok(value)
}
pub(crate) fn same_scope(plaintext: &[u8], scope: &[u8]) -> io::Result<bool> {
    if plaintext.len() < MAGIC.len() || !plaintext.starts_with(MAGIC) || plaintext.len() > MAGIC.len() + MAX_SCOPE {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid protected scope"));
    }
    Ok(&plaintext[MAGIC.len()..] == scope)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn envelope_has_context_and_rejects_bad_or_oversized_data() {
        let value = envelope(b"scope").unwrap();
        assert!(same_scope(&value, b"scope").unwrap());
        assert!(!same_scope(&value, b"changed").unwrap());
        assert!(same_scope(b"not a scope", b"scope").is_err());
        assert!(envelope(&vec![0; MAX_SCOPE + 1]).is_err());
    }
}
