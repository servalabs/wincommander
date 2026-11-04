//! Cross-version check for previously issued Ed25519 signatures.

#[test]
fn existing_and_new_ed25519_signatures_interoperate() {
    let seed = [0x42; 32];
    let message = b"existing WinCommander signed artifact fixture";
    let old = ed25519_dalek_v2::SigningKey::from_bytes(&seed);
    let new = ed25519_dalek::SigningKey::from_bytes(&seed);
    let old_signature: ed25519_dalek_v2::Signature = ed25519_dalek_v2::Signer::sign(&old, message);
    let new_signature: ed25519_dalek::Signature = ed25519_dalek::Signer::sign(&new, message);
    assert_eq!(old.verifying_key().to_bytes(), new.verifying_key().to_bytes());
    assert_eq!(old_signature.to_bytes(), new_signature.to_bytes());
    assert!(ed25519_dalek::Verifier::verify(
        &new.verifying_key(),
        message,
        &ed25519_dalek::Signature::from_bytes(&old_signature.to_bytes())
    )
    .is_ok());
    assert!(ed25519_dalek_v2::Verifier::verify(
        &old.verifying_key(),
        message,
        &ed25519_dalek_v2::Signature::from_bytes(&new_signature.to_bytes())
    )
    .is_ok());
    assert!(ed25519_dalek::Verifier::verify(
        &new.verifying_key(),
        b"tampered",
        &ed25519_dalek::Signature::from_bytes(&old_signature.to_bytes())
    )
    .is_err());
}
