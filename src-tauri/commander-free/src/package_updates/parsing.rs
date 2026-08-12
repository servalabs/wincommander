use serde_json::Value;

pub(super) fn parse_text(manager: &str, text: &str) -> Vec<(String, String, String)> {
    text.lines()
        .filter_map(|line| {
            let fields: Vec<_> = if manager == "chocolatey" {
                line.split('|').collect()
            } else {
                line.split_whitespace().collect()
            };
            match manager {
                "chocolatey"
                    if fields.len() >= 3 && !fields[0].eq_ignore_ascii_case("chocolatey") =>
                {
                    Some((fields[0].into(), fields[1].into(), fields[2].into()))
                }
                "scoop" if fields.len() >= 3 && fields[0] != "Name" && !line.starts_with('-') => {
                    Some((fields[0].into(), fields[1].into(), fields[2].into()))
                }
                "winget"
                    if fields.len() >= 4
                        && !fields[0].eq_ignore_ascii_case("name")
                        && !line.starts_with('-') =>
                {
                    let last = fields.len();
                    Some((
                        fields[last - 4].into(),
                        fields[last - 3].into(),
                        fields[last - 2].into(),
                    ))
                }
                _ => None,
            }
        })
        .collect()
}

pub(super) fn parse_npm(text: &str) -> Result<Vec<(String, String, String)>, String> {
    let value: Value = serde_json::from_str(text)
        .map_err(|_| "npm returned invalid update inventory".to_string())?;
    Ok(value
        .as_object()
        .into_iter()
        .flat_map(|items| items.iter())
        .filter_map(|(name, item)| {
            Some((
                name.clone(),
                item.get("current")?.as_str()?.into(),
                item.get("latest")?.as_str()?.into(),
            ))
        })
        .collect())
}
