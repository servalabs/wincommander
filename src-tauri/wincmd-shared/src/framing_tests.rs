use crate::{read_envelope_limited, write_envelope, Envelope, MAX_PAYLOAD_BYTES};
use std::io::ErrorKind;
use tokio::io::{duplex, AsyncWriteExt};

#[tokio::test]
async fn custom_cap_rejects_length_before_reading_or_allocating_the_body() {
    let (mut sender, mut receiver) = duplex(32);
    sender.write_u32_le(1025).await.unwrap();
    assert_eq!(
        read_envelope_limited(&mut receiver, 1024)
            .await
            .unwrap_err()
            .kind(),
        ErrorKind::InvalidData
    );
}

#[tokio::test]
async fn caller_cannot_raise_the_protocol_maximum() {
    let (mut sender, mut receiver) = duplex(32);
    sender.write_u32_le(MAX_PAYLOAD_BYTES + 1).await.unwrap();
    assert_eq!(
        read_envelope_limited(&mut receiver, u32::MAX)
            .await
            .unwrap_err()
            .kind(),
        ErrorKind::InvalidData
    );
}

#[tokio::test]
async fn limited_reader_preserves_valid_frame_decoding() {
    let (mut sender, mut receiver) = duplex(1024);
    write_envelope(&mut sender, &Envelope::Bye).await.unwrap();
    assert!(matches!(
        read_envelope_limited(&mut receiver, 1024).await.unwrap(),
        Envelope::Bye
    ));
}
