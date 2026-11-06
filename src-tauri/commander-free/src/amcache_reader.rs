//! Read an Amcache hive from a private VSS snapshot. Never writes to the hive.
use notatin::{cell_value::CellValue, log::LogCode, parser_builder::ParserBuilder};
use serde_json::{json, Map, Value};
use std::{fs, io::Read, path::Path};

const MAX_FILE: u64 = 256 * 1024 * 1024;
const MAX_RECORDS: usize = 200_000;
const CATEGORIES: [&str; 5] = [
    "InventoryApplication",
    "InventoryApplicationFile",
    "InventoryApplicationShortcut",
    "InventoryDeviceContainer",
    "InventoryDriverBinary",
];

fn read_bounded(path: &Path) -> Result<Vec<u8>, String> {
    let mut f = fs::File::open(path)
        .map_err(|_| "A snapshot registry file could not be read".to_owned())?;
    let size = f
        .metadata()
        .map_err(|_| "A snapshot registry file could not be inspected".to_owned())?
        .len();
    if size > MAX_FILE {
        return Err("A snapshot registry file exceeds the scan limit".into());
    }
    let mut bytes = Vec::new();
    f.by_ref()
        .take(MAX_FILE + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "A snapshot registry file could not be read".to_owned())?;
    if bytes.len() as u64 != size {
        return Err("A snapshot registry file changed or was truncated".into());
    }
    Ok(bytes)
}

fn u32_at(b: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        b.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

fn header_of_type(b: &[u8], file_type: u32) -> Result<(u32, u32), String> {
    if b.len() < 512 || &b[..4] != b"regf" || u32_at(b, 28) != Some(file_type) {
        return Err("Invalid registry hive header".into());
    }
    let mut checksum = 0u32;
    for offset in (0..508).step_by(4) {
        checksum ^= u32_at(b, offset).ok_or("Invalid registry hive header")?;
    }
    if checksum == u32::MAX {
        checksum = u32::MAX - 1;
    } else if checksum == 0 {
        checksum = 1;
    }
    if u32_at(b, 508) != Some(checksum) {
        return Err("Registry hive header checksum is invalid".into());
    }
    Ok((u32_at(b, 4).unwrap(), u32_at(b, 8).unwrap()))
}

fn header(b: &[u8]) -> Result<(u32, u32), String> {
    header_of_type(b, 0)
}

/// Require contiguous log entries beginning at or after the hive's last
/// committed sequence, per the new-format recovery rules. notatin verifies
/// entry hashes and its warnings are checked after replay.
fn validate_log_sequences(logs: &[Vec<u8>], primary: u32, secondary: u32) -> Result<u32, String> {
    let mut seqs = Vec::new();
    for log in logs {
        let (log_primary, log_secondary) = header_of_type(log, 6)?;
        if log_primary != log_secondary {
            return Err("Registry transaction log header is inconsistent".into());
        }
        let mut pos = 512usize;
        let mut first = true;
        while pos + 40 <= log.len() && &log[pos..pos + 4] == b"HvLE" {
            let size = u32_at(log, pos + 4).ok_or("Invalid transaction log entry")? as usize;
            if size < 40
                || size % 512 != 0
                || pos
                    .checked_add(size)
                    .filter(|end| *end <= log.len())
                    .is_none()
            {
                return Err("Registry transaction log is truncated".into());
            }
            let seq = u32_at(log, pos + 12).ok_or("Invalid transaction log entry")?;
            if first && seq != log_primary {
                return Err("Registry log sequence does not match its header".into());
            }
            first = false;
            let bins = u32_at(log, pos + 16).ok_or("Invalid transaction log entry")? as usize;
            let pages = u32_at(log, pos + 20).ok_or("Invalid transaction log entry")? as usize;
            let refs_end = 40usize
                .checked_add(
                    pages
                        .checked_mul(8)
                        .ok_or("Invalid transaction log page count")?,
                )
                .filter(|n| *n <= size)
                .ok_or("Invalid transaction log page count")?;
            let mut data_end = refs_end;
            for i in 0..pages {
                let page_offset =
                    u32_at(log, pos + 40 + i * 8).ok_or("Invalid transaction log page")? as usize;
                let page_size =
                    u32_at(log, pos + 44 + i * 8).ok_or("Invalid transaction log page")? as usize;
                if page_offset
                    .checked_add(page_size)
                    .filter(|end| *end <= bins)
                    .is_none()
                {
                    return Err("Registry transaction log page is out of bounds".into());
                }
                data_end = data_end
                    .checked_add(page_size)
                    .filter(|end| *end <= size)
                    .ok_or("Registry transaction log page is truncated")?;
            }
            seqs.push(seq);
            pos += size;
        }
        if first {
            return Err("Registry transaction log has no readable entries".into());
        }
        // Windows may retain old bytes after the final active entry. An HvLE
        // signature at the next boundary must still form a complete entry.
        if log.get(pos..pos + 4) == Some(b"HvLE") {
            return Err("Registry transaction log has a truncated entry".into());
        }
    }
    seqs.sort_unstable();
    if seqs.windows(2).any(|w| w[0] == w[1]) {
        return Err("Registry transaction logs have overlapping entries".into());
    }
    let relevant: Vec<u32> = seqs.into_iter().filter(|n| *n >= secondary).collect();
    if relevant.is_empty()
        || relevant.last().copied().unwrap_or(0) < primary
        || relevant
            .windows(2)
            .any(|w| w[0].checked_add(1) != Some(w[1]))
    {
        return Err(format!("Registry transaction logs do not cover the required sequence or contain a gap (hive {secondary}–{primary}, available {}–{}).", relevant.first().map(u32::to_string).unwrap_or_else(|| "none".into()), relevant.last().map(u32::to_string).unwrap_or_else(|| "none".into())));
    }
    Ok(*relevant.last().unwrap())
}

fn parse_snapshot(hive: Vec<u8>, logs: Vec<Vec<u8>>) -> Result<notatin::parser::Parser, String> {
    let (primary, secondary) = header(&hive)?;
    let mut builder = ParserBuilder::from_file(std::io::Cursor::new(hive));
    let expected_sequence = if primary != secondary {
        if logs.len() != 2 {
            return Err("Registry transaction logs are unavailable".into());
        }
        let expected = validate_log_sequences(&logs, primary, secondary)?;
        for log in logs {
            builder.with_transaction_log(std::io::Cursor::new(log));
        }
        Some(expected)
    } else {
        None
    };
    let parser = builder
        .build()
        .map_err(|_| "Registry snapshot could not be parsed".to_owned())?;
    let parse_logs = parser.get_parse_logs().get().cloned().unwrap_or_default();
    let mut replay_confirmed = expected_sequence.is_none();
    for log in parse_logs {
        if log.code == LogCode::Info {
            if let Some(expected) = expected_sequence {
                let marker = format!(
                    "Applied transaction log(s). Sequence numbers have been updated to {expected}."
                );
                if log.text.starts_with(&marker) {
                    replay_confirmed = true;
                }
            }
        } else if !(log.code == LogCode::WarningBaseBlock
            && log.text.starts_with("Hive requires recovery: primary"))
        {
            return Err("Registry snapshot or transaction logs could not be verified".into());
        }
    }
    if !replay_confirmed {
        return Err("Registry transaction log replay could not be verified".into());
    }
    Ok(parser)
}

pub fn read_amcache_snapshot(path: &Path) -> Result<Value, String> {
    let hive = read_bounded(path)?;
    let (primary, secondary) = header(&hive)?;
    let mut logs = Vec::new();
    if primary != secondary {
        for suffix in [".LOG1", ".LOG2"] {
            let mut log_path = path.as_os_str().to_os_string();
            log_path.push(suffix);
            logs.push(read_bounded(Path::new(&log_path))?);
        }
    }
    let mut parser = parse_snapshot(hive, logs)?;
    let mut root = parser
        .get_root_key()
        .map_err(|_| "Registry root could not be read")?
        .ok_or("Registry root is unavailable")?;
    let root_children = root.read_sub_keys(&mut parser);
    if root.logs.has_logs() || root_children.len() != root.detail.number_of_sub_keys() as usize {
        return Err("Registry root inventory is incomplete".into());
    }
    let mut inventory = root_children
        .into_iter()
        .find(|k| k.key_name.eq_ignore_ascii_case("Root"))
        .ok_or("Amcache inventory root is unavailable")?;
    let category_nodes = inventory.read_sub_keys(&mut parser);
    if inventory.logs.has_logs()
        || category_nodes.len() != inventory.detail.number_of_sub_keys() as usize
    {
        return Err("Amcache categories could not be read completely".into());
    }
    if !category_nodes.iter().any(|node| {
        CATEGORIES
            .iter()
            .any(|name| node.key_name.eq_ignore_ascii_case(name))
    }) {
        return Err("No recognizable Amcache inventory categories were found".into());
    }
    let mut entries = Vec::new();
    let mut total = 0usize;
    for category in CATEGORIES {
        let mut sample = Vec::new();
        if let Some(mut node) = category_nodes
            .iter()
            .find(|k| k.key_name.eq_ignore_ascii_case(category))
            .cloned()
        {
            let children = node.read_sub_keys(&mut parser);
            if node.logs.has_logs() || children.len() != node.detail.number_of_sub_keys() as usize {
                return Err("An Amcache category could not be read completely".into());
            }
            total = total
                .checked_add(children.len())
                .filter(|n| *n <= MAX_RECORDS)
                .ok_or("Amcache inventory exceeds the scan limit")?;
            for child in children {
                if child.key_name.is_empty() || child.logs.has_logs() {
                    return Err("An Amcache entry could not be read completely".into());
                }
                let mut values = Map::new();
                if child.value_iter().count() != child.detail.number_of_key_values() as usize {
                    return Err("An Amcache entry has incomplete values".into());
                }
                for value in child.value_iter() {
                    let (content, warnings) = value.get_content();
                    if warnings.is_some() || matches!(content, CellValue::Error) {
                        return Err("An Amcache value could not be decoded".into());
                    }
                    let v = match content {
                        CellValue::None => Value::Null,
                        CellValue::String(s) => json!(s),
                        CellValue::MultiString(s) => json!(s),
                        CellValue::U32(n) => json!(n),
                        CellValue::I32(n) => json!(n),
                        CellValue::U64(n) => json!(n),
                        CellValue::I64(n) => json!(n),
                        CellValue::Binary(b) => json!(b),
                        CellValue::Error => unreachable!(),
                    };
                    values.insert(value.get_pretty_name(), v);
                }
                let first = |names: &[&str]| {
                    names
                        .iter()
                        .find_map(|name| values.get(*name).and_then(Value::as_str))
                        .unwrap_or("")
                        .to_string()
                };
                let name = first(&["Name", "ProductName", "BinFileVersion"]);
                let path = first(&["LowerCaseLongPath", "LongPathHash"]);
                sample.push(
                    json!({"id":child.key_name,"name":name,"path":path,"registryValues":values}),
                );
            }
        }
        entries.push(json!({"category":category,"count":sample.len(),"sample":sample}));
    }
    for log in parser.get_parse_logs().get().into_iter().flatten() {
        if log.code != LogCode::Info
            && !(log.code == LogCode::WarningBaseBlock
                && log.text.starts_with("Hive requires recovery: primary"))
        {
            return Err("Registry snapshot inventory could not be verified".into());
        }
    }
    Ok(json!({"status":"complete","entries":entries,"total":total}))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fix_checksum(b: &mut [u8]) {
        let mut n = 0;
        for offset in (0..508).step_by(4) {
            n ^= u32_at(b, offset).unwrap();
        }
        if n == u32::MAX {
            n -= 1;
        } else if n == 0 {
            n = 1;
        }
        b[508..512].copy_from_slice(&n.to_le_bytes());
    }
    fn synthetic_log(sequences: &[u32]) -> Vec<u8> {
        let mut l = vec![0u8; 512 + 512 * sequences.len()];
        l[..4].copy_from_slice(b"regf");
        l[4..8].copy_from_slice(&sequences[0].to_le_bytes());
        l[8..12].copy_from_slice(&sequences[0].to_le_bytes());
        l[28..32].copy_from_slice(&6u32.to_le_bytes());
        fix_checksum(&mut l);
        for (i, seq) in sequences.iter().enumerate() {
            let p = 512 * (i + 1);
            l[p..p + 4].copy_from_slice(b"HvLE");
            l[p + 4..p + 8].copy_from_slice(&512u32.to_le_bytes());
            l[p + 12..p + 16].copy_from_slice(&seq.to_le_bytes());
        }
        l
    }
    #[test]
    fn rejects_invalid_header() {
        assert!(header(&[0; 512]).is_err());
    }
    #[test]
    fn rejects_missing_and_gapped_logs() {
        assert!(validate_log_sequences(&[], 3, 2).is_err());
        let l = synthetic_log(&[2, 4]);
        assert!(validate_log_sequences(&[l], 4, 2).is_err());
    }
    #[test]
    fn accepts_contiguous_sequences_in_preflight_only() {
        let l = synthetic_log(&[2, 3]);
        assert!(validate_log_sequences(&[l], 3, 2).is_ok());
        // Windows permits the first log sequence to be greater than the
        // secondary sequence, because earlier entries may already be applied.
        assert!(validate_log_sequences(&[synthetic_log(&[4, 5])], 3, 2).is_ok());
    }
    #[test]
    #[ignore = "requires WINCOMMANDER_TEST_HIVE_DIR with upstream notatin test data"]
    fn real_log_replay_when_fixture_is_available() {
        let dir = std::env::var_os("WINCOMMANDER_TEST_HIVE_DIR")
            .expect("fixture directory must be supplied");
        let dir = Path::new(&dir);
        let hive = fs::read(dir.join("system")).unwrap();
        let logs = vec![
            fs::read(dir.join("system.log1")).unwrap(),
            fs::read(dir.join("system.log2")).unwrap(),
        ];
        let parser =
            parse_snapshot(hive.clone(), logs.clone()).expect("unmodified fixture logs must replay");
        let (keys, values) = parser.count_all_keys_and_values(None);
        assert_eq!((keys, values), (45587, 108178));
        let mut corrupted = logs;
        let pages = u32_at(&corrupted[0], 512 + 20).unwrap() as usize;
        let first_page_byte = 512 + 40 + pages * 8;
        corrupted[0][first_page_byte] ^= 1;
        assert!(parse_snapshot(hive, corrupted).is_err());
    }
}
