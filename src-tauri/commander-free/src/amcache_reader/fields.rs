// SPDX-License-Identifier: AGPL-3.0-or-later
use serde_json::{Map, Value};

// Hashes and file versions are not application paths or names; retain those
// original values in registryValues without mislabeling them as display fields.
pub(super) fn inventory_text_fields(values: &Map<String, Value>) -> (String, String) {
    let first = |names: &[&str]| {
        names
            .iter()
            .find_map(|name| {
                values.iter().find_map(|(key, value)| {
                    if !key.eq_ignore_ascii_case(name) {
                        return None;
                    }
                    value.as_str().filter(|value| !value.trim().is_empty())
                })
            })
            .unwrap_or("")
            .to_string()
    };
    (
        first(&["Name", "ProductName", "FileName"]),
        first(&["LowerCaseLongPath", "LongPath"]),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn version_and_hash_are_not_names_or_paths() {
        let mut values = Map::new();
        values.insert("LongPathHash".into(), json!("abc123"));
        values.insert("BinFileVersion".into(), json!("1.2.3"));
        assert_eq!(
            inventory_text_fields(&values),
            (String::new(), String::new())
        );
        values.insert("productname".into(), json!("Example app"));
        values.insert("LowerCaseLongPath".into(), json!(r"c:\apps\example.exe"));
        assert_eq!(
            inventory_text_fields(&values),
            ("Example app".into(), r"c:\apps\example.exe".into())
        );
    }

    #[test]
    fn blank_and_non_string_values_do_not_hide_a_real_field() {
        let mut values = Map::new();
        values.insert("Name".into(), json!("  "));
        values.insert("name".into(), json!("actual"));
        values.insert("LowerCaseLongPath".into(), json!(123));
        values.insert("LongPath".into(), json!(r"c:\apps\real.exe"));
        assert_eq!(
            inventory_text_fields(&values),
            ("actual".into(), r"c:\apps\real.exe".into())
        );
    }
}
