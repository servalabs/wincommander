#[cfg(test)]
use crate::types::DocProps;
use crate::types::{Chunk, ChunkField, ExtractedDoc};

const CHUNK_SIZE: usize = 512;

/// Split an extracted document into indexable chunks.
///
/// Returns: chunk 0 = Title, then sequential Body chunks of up to 512 chars
/// each, split only on UTF-8 char boundaries.
pub fn chunk_doc(doc: &ExtractedDoc) -> Vec<Chunk> {
    let mut chunks = Vec::new();

    // Title chunk (ordinal 0).
    chunks.push(Chunk {
        doc_id: doc.meta.doc_id,
        field: ChunkField::Title,
        ordinal: 0,
        text: doc.title.clone(),
    });

    // Body chunks at CHUNK_SIZE char boundaries.
    let body = &doc.body;
    let mut start = 0;
    let mut ordinal: u32 = 1;
    while start < body.len() {
        let mut end = (start + CHUNK_SIZE).min(body.len());
        while !body.is_char_boundary(end) {
            end -= 1;
        }
        chunks.push(Chunk {
            doc_id: doc.meta.doc_id,
            field: ChunkField::Body,
            ordinal,
            text: body[start..end].to_owned(),
        });
        start = end;
        ordinal += 1;
    }
    chunks
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::FileMeta;
    use std::path::PathBuf;

    fn make_doc(title: &str, body: &str) -> ExtractedDoc {
        ExtractedDoc {
            meta: FileMeta {
                doc_id: 42,
                path: PathBuf::from("test.txt"),
                name: title.to_owned(),
                ext: "txt".to_owned(),
                mtime: 0,
                size: 0,
            },
            title: title.to_owned(),
            body: body.to_owned(),
            // KT: props not exercised by chunking logic — DocProps ships in a
            // later phase's ExtractedDoc extension, chunk_doc doesn't read it.
            props: DocProps::default(),
        }
    }

    #[test]
    fn chunk_0_is_title() {
        let doc = make_doc("My Document", "some body text");
        let chunks = chunk_doc(&doc);
        assert_eq!(chunks[0].field, ChunkField::Title);
        assert_eq!(chunks[0].ordinal, 0);
        assert_eq!(chunks[0].text, "My Document");
    }

    #[test]
    fn body_chunks_reconstruct_original() {
        let body = "x".repeat(2000);
        let doc = make_doc("title", &body);
        let chunks = chunk_doc(&doc);
        let reconstructed: String = chunks
            .iter()
            .filter(|c| c.field == ChunkField::Body)
            .map(|c| c.text.as_str())
            .collect();
        assert_eq!(reconstructed, body);
    }

    #[test]
    fn ordinals_are_sequential() {
        let body = "y".repeat(1600);
        let doc = make_doc("t", &body);
        let chunks = chunk_doc(&doc);
        for (i, chunk) in chunks.iter().enumerate() {
            assert_eq!(chunk.ordinal, i as u32);
        }
    }

    #[test]
    fn long_body_splits_into_multiple_chunks() {
        let body = "z".repeat(600); // > 512 chars → must produce 2 body chunks
        let doc = make_doc("t", &body);
        let chunks = chunk_doc(&doc);
        let body_chunks: Vec<_> = chunks
            .iter()
            .filter(|c| c.field == ChunkField::Body)
            .collect();
        assert!(
            body_chunks.len() >= 2,
            "expected ≥2 body chunks, got {}",
            body_chunks.len()
        );
    }

    #[test]
    fn empty_body_produces_only_title_chunk() {
        let doc = make_doc("title only", "");
        let chunks = chunk_doc(&doc);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].field, ChunkField::Title);
    }

    #[test]
    fn doc_id_propagated_to_all_chunks() {
        let body = "a".repeat(1000);
        let doc = make_doc("t", &body);
        let chunks = chunk_doc(&doc);
        for c in &chunks {
            assert_eq!(c.doc_id, 42);
        }
    }

    #[test]
    fn unicode_body_splits_on_char_boundary() {
        // Each '€' is 3 bytes; fill past 512 bytes so the naive byte boundary
        // would land inside a multibyte char.
        let body = "€".repeat(200); // 600 bytes total
        let doc = make_doc("t", &body);
        let chunks = chunk_doc(&doc);
        // All chunk texts must be valid strings (they are, since we built them
        // from &str slices on char boundaries). Reconstruct to verify.
        let reconstructed: String = chunks
            .iter()
            .filter(|c| c.field == ChunkField::Body)
            .map(|c| c.text.as_str())
            .collect();
        assert_eq!(reconstructed, body);
    }
}
