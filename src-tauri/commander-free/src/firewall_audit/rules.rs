use super::Rule;
use std::collections::HashMap;

pub(super) fn parse_rules(text: &str) -> Vec<Rule> {
    let mut rules = Vec::new();
    let mut fields = HashMap::<String, String>::new();
    let flush = |fields: &mut HashMap<String, String>, rules: &mut Vec<Rule>| {
        let Some(name) = fields.remove("Rule Name") else {
            return;
        };
        let Some(enabled_text) = fields.remove("Enabled") else {
            fields.clear();
            return;
        };
        let Some(action) = fields.remove("Action") else {
            fields.clear();
            return;
        };
        let enabled = enabled_text.eq_ignore_ascii_case("yes");
        let program = fields.remove("Program").unwrap_or_default();
        if !name.is_empty() {
            rules.push(Rule {
                name,
                enabled,
                action,
                program,
            });
        }
        fields.clear();
    };
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("Rule Name:") && fields.contains_key("Rule Name") {
            flush(&mut fields, &mut rules);
        }
        if let Some((key, value)) = line.split_once(':') {
            if matches!(key, "Rule Name" | "Enabled" | "Action" | "Program") {
                fields.insert(key.into(), value.trim().into());
            }
        }
    }
    flush(&mut fields, &mut rules);
    rules
}

pub(super) fn eligible(rule: &Rule) -> bool {
    !protected(rule) && (!rule.enabled || rule.action.eq_ignore_ascii_case("allow"))
}

fn protected(rule: &Rule) -> bool {
    let name = rule.name.to_ascii_lowercase();
    let program = rule.program.to_ascii_lowercase();
    let app = crate::paths::app_display_name().to_ascii_lowercase();
    name.contains("windows")
        || name.contains("microsoft")
        || name.contains("defender")
        || name.contains("wincommander")
        || name.contains(&app)
        || program.contains("\\windows\\")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_english_netsh_rule() {
        let rules = parse_rules(
            "Rule Name: Example\nEnabled: No\nAction: Allow\nProgram: C:\\Example.exe\n",
        );
        assert_eq!(rules.len(), 1);
        assert!(!rules[0].enabled);
    }

    #[test]
    fn excludes_windows_and_wincommander_rules() {
        assert!(!eligible(&Rule {
            name: "Windows Update".into(),
            enabled: true,
            action: "Allow".into(),
            program: String::new()
        }));
        assert!(!eligible(&Rule {
            name: "WinCommander-KillSwitch-Out".into(),
            enabled: true,
            action: "Block".into(),
            program: String::new()
        }));
    }
}
