//! Bounded incremental NDJSON tail. Invalid records never become partial events.
use std::io::{self, Read, Seek, SeekFrom};

const POLL_BYTES: usize = 64 * 1024;
const LINE_BYTES: usize = 8 * 1024;

#[derive(Default)]
pub(crate) struct EventTail {
    pub(crate) offset: u64,
    key: Option<(u64, u64)>,
    partial: Vec<u8>,
    skipping: bool,
    anchor: Vec<u8>,
}

pub(crate) struct Batch {
    pub(crate) lines: Vec<String>,
    pub(crate) dropped: bool,
}

impl EventTail {
    pub(crate) fn new(offset: u64) -> Self { Self { offset, ..Self::default() } }

    pub(crate) fn poll<R: Read + Seek>(&mut self, reader: &mut R, len: u64, key: (u64, u64)) -> io::Result<Batch> {
        let mut batch = Batch { lines: Vec::new(), dropped: false };
        if self.key.is_some_and(|previous| previous != key) {
            // Replacement content may be a replayed archive. Start at its EOF.
            self.offset = len;
            self.partial.clear();
            self.anchor.clear();
            self.skipping = false;
            batch.dropped = true;
        } else if len < self.offset || !self.anchor_matches(reader)? {
            self.offset = 0;
            self.partial.clear();
            self.anchor.clear();
            self.skipping = false;
        }
        self.key = Some(key);
        reader.seek(SeekFrom::Start(self.offset))?;
        let mut bytes = vec![0; POLL_BYTES];
        let count = reader.read(&mut bytes)?;
        bytes.truncate(count);
        self.offset += count as u64;
        for byte in bytes {
            if byte == b'\n' {
                if !self.skipping {
                    match String::from_utf8(std::mem::take(&mut self.partial)) {
                        Ok(line) => batch.lines.push(line),
                        Err(_) => batch.dropped = true,
                    }
                }
                self.partial.clear();
                self.skipping = false;
            } else if !self.skipping {
                if self.partial.len() == LINE_BYTES {
                    self.partial.clear();
                    self.skipping = true;
                    batch.dropped = true;
                } else {
                    self.partial.push(byte);
                }
            }
        }
        let anchor_len = self.offset.min(64) as usize;
        reader.seek(SeekFrom::Start(self.offset - anchor_len as u64))?;
        self.anchor.resize(anchor_len, 0);
        reader.read_exact(&mut self.anchor)?;
        Ok(batch)
    }

    fn anchor_matches<R: Read + Seek>(&self, reader: &mut R) -> io::Result<bool> {
        if self.anchor.is_empty() { return Ok(true); }
        reader.seek(SeekFrom::Start(self.offset - self.anchor.len() as u64))?;
        let mut actual = vec![0; self.anchor.len()];
        match reader.read_exact(&mut actual) {
            Ok(()) => Ok(actual == self.anchor),
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => Ok(false),
            Err(error) => Err(error),
        }
    }
}

pub(crate) fn file_key(file: &std::fs::File) -> io::Result<(u64, u64)> {
    #[cfg(windows)] {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION};
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok((info.dwVolumeSerialNumber as u64, ((info.nFileIndexHigh as u64) << 32) | info.nFileIndexLow as u64))
    }
    #[cfg(not(windows))] {
        use std::os::unix::fs::MetadataExt;
        let metadata = file.metadata()?;
        Ok((metadata.dev(), metadata.ino()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn poll(tail: &mut EventTail, bytes: &[u8], key: (u64,u64)) -> Batch {
        tail.poll(&mut Cursor::new(bytes), bytes.len() as u64, key).unwrap()
    }

    #[test]
    fn split_utf8_and_json_are_retained_until_newline() {
        let mut tail = EventTail::default();
        assert!(poll(&mut tail, &[b'"',0xc3], (1,1)).lines.is_empty());
        assert_eq!(poll(&mut tail, &[b'"',0xc3,0xa9,b'"',b'\n'], (1,1)).lines, vec!["\"é\""]);
        assert!(poll(&mut tail, &[b'"',0xc3,0xa9,b'"',b'\n'], (1,1)).lines.is_empty());
    }

    #[test]
    fn oversized_line_is_discarded_without_losing_next_record() {
        let mut tail = EventTail::default();
        let mut bytes = vec![b'x'; LINE_BYTES + 1];
        bytes.extend_from_slice(b"\n{}\n");
        let batch = poll(&mut tail, &bytes, (1,1));
        assert!(batch.dropped);
        assert_eq!(batch.lines, vec!["{}"]);
        assert!(tail.partial.len() <= LINE_BYTES);
    }

    #[test]
    fn poll_is_bounded_and_replacement_is_not_replayed() {
        let mut tail = EventTail::default();
        let bytes = vec![b'x'; POLL_BYTES * 2];
        poll(&mut tail, &bytes, (1,1));
        assert_eq!(tail.offset, POLL_BYTES as u64);
        assert!(poll(&mut tail, b"{}\n", (1,2)).lines.is_empty());
        assert_eq!(poll(&mut tail, b"{}\n1\n", (1,2)).lines, vec!["1"]);
    }

    #[test]
    fn truncate_and_regrow_detected_and_invalid_utf8_discarded() {
        let mut tail = EventTail::default();
        assert_eq!(poll(&mut tail, b"old\n", (1,1)).lines, vec!["old"]);
        assert_eq!(poll(&mut tail, b"new\nmore\n", (1,1)).lines, vec!["new", "more"]);
        let mut bytes = b"new\nmore\n".to_vec();
        bytes.extend_from_slice(&[0xff,b'\n']);
        assert!(poll(&mut tail, &bytes, (1,1)).dropped);
    }
}
