// SPDX-License-Identifier: AGPL-3.0-or-later
//! A ZIP created independently of Rust remains readable after bzip2 updates.
use std::io::{Cursor, Read};

#[test]
fn reads_existing_bzip2_compressed_zip() {
    let bytes = include_bytes!("fixtures/legacy-bzip2.zip");
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut entry = archive.by_name("existing.txt").unwrap();
    let mut content = String::new();
    entry.read_to_string(&mut content).unwrap();
    assert_eq!(content, "Existing archive content survives the upgrade.\n");
}
