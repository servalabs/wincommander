// SPDX-License-Identifier: AGPL-3.0-or-later
use crate::Encoding;
use base64::{
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
    Engine,
};
pub(crate) struct Decoded {
    pub text: String,
    pub range: std::ops::Range<usize>,
    pub encoding: Encoding,
}
pub(crate) fn decode_candidates(text: &str) -> Vec<Decoded> {
    let mut decoded = Vec::new();
    let mut cursor = 0;
    let bytes = text.as_bytes();
    while cursor < bytes.len() && decoded.len() < 64 {
        if !is_encoded(bytes[cursor]) {
            cursor += 1;
            continue;
        }
        let start = cursor;
        while cursor < bytes.len() && is_encoded(bytes[cursor]) {
            cursor += 1;
        }
        let token = &text[start..cursor];
        if !(16..=4096).contains(&token.len()) {
            continue;
        }
        let (data, encoding) =
            if token.len().is_multiple_of(2) && token.bytes().all(|b| b.is_ascii_hexdigit()) {
                let data: Option<Vec<u8>> = token
                    .as_bytes()
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|pair| Some((hex(pair[0])? << 4) | hex(pair[1])?))
                    .collect();
                (data, Encoding::Hex)
            } else {
                (
                    STANDARD
                        .decode(token)
                        .or_else(|_| URL_SAFE_NO_PAD.decode(token))
                        .ok(),
                    Encoding::Base64,
                )
            };
        if let Some(data) = data {
            if let Ok(text) = String::from_utf8(data) {
                if text
                    .chars()
                    .all(|c| !c.is_control() || matches!(c, '\n' | '\r' | '\t'))
                {
                    decoded.push(Decoded {
                        text,
                        range: start..cursor,
                        encoding,
                    });
                }
            }
        }
    }
    decoded
}
fn hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}
fn is_encoded(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'=' | b'-' | b'_')
}
