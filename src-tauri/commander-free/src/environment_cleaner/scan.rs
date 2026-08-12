use super::registry::{self, Scope};
use super::{CachedEntry, EnvironmentFinding, EnvironmentScan, DIRECTORY_VARIABLES};
use std::collections::HashMap;
use std::path::Path;
use uuid::Uuid;

pub(super) fn scan_environment() -> (EnvironmentScan, HashMap<String, CachedEntry>) {
    let mut merged = HashMap::new();
    let mut values_by_scope = Vec::new();
    for scope in [Scope::Machine, Scope::User] {
        let values = registry::values(scope).unwrap_or_default();
        for value in &values {
            merged.insert(value.name.to_ascii_lowercase(), value.text.clone());
        }
        values_by_scope.push((scope, values));
    }
    let mut public = Vec::new();
    let mut cached = HashMap::new();
    let mut skipped = 0;
    for (scope, values) in values_by_scope {
        for value in values {
            if value.name.eq_ignore_ascii_case("path") {
                for entry in value
                    .text
                    .split(';')
                    .map(str::trim)
                    .filter(|entry| !entry.is_empty())
                {
                    let Some(expanded) = expand_variables(entry, &merged) else {
                        skipped += 1;
                        continue;
                    };
                    if !Path::new(&expanded).exists() {
                        let id = Uuid::new_v4().to_string();
                        public.push(EnvironmentFinding {
                            id: id.clone(),
                            scope: scope.label().into(),
                            variable: "PATH".into(),
                            value: entry.into(),
                            kind: "missingPathEntry".into(),
                        });
                        cached.insert(
                            id,
                            CachedEntry::Path {
                                scope,
                                old_path: value.text.clone(),
                                missing_entry: entry.into(),
                                value_type: value.value_type,
                            },
                        );
                    }
                }
            } else if DIRECTORY_VARIABLES
                .iter()
                .any(|name| value.name.eq_ignore_ascii_case(name))
            {
                let Some(expanded) = expand_variables(&value.text, &merged) else {
                    skipped += 1;
                    continue;
                };
                if !Path::new(&expanded).exists() {
                    let id = Uuid::new_v4().to_string();
                    public.push(EnvironmentFinding {
                        id: id.clone(),
                        scope: scope.label().into(),
                        variable: value.name.clone(),
                        value: value.text.clone(),
                        kind: "missingDirectoryVariable".into(),
                    });
                    cached.insert(id, CachedEntry::Variable { scope, value });
                }
            }
        }
    }
    public.sort_by(|left, right| {
        (&left.scope, &left.variable, &left.value).cmp(&(
            &right.scope,
            &right.variable,
            &right.value,
        ))
    });
    (
        EnvironmentScan {
            entries: public,
            skipped_entries: skipped,
        },
        cached,
    )
}

pub(super) fn expand_variables(value: &str, values: &HashMap<String, String>) -> Option<String> {
    let mut result = String::new();
    let mut remaining = value;
    while let Some(start) = remaining.find('%') {
        result.push_str(&remaining[..start]);
        let after = &remaining[start + 1..];
        let end = after.find('%')?;
        let name = &after[..end];
        result.push_str(values.get(&name.to_ascii_lowercase())?);
        remaining = &after[end + 1..];
    }
    result.push_str(remaining);
    (!result.contains('%')).then_some(result)
}
