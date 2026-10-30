// SPDX-License-Identifier: AGPL-3.0-or-later

pub(super) fn app_category(id: &str) -> &'static str {
    match id {
        "vscode" | "jetbrains" | "npm" | "yarn" | "pnpm" | "bun" | "pip" | "nuget" | "cargo"
        | "go" | "gradle" | "maven" | "composer" | "docker" | "cursor" | "cursor-partitions"
        | "postman" | "github-desktop" | "android-studio" | "conda" | "unity" | "visual-studio"
        | "ruby-gems" | "deno" | "node-gyp" | "electron-builder" | "windsurf" | "scoop"
        | "sublime-text" => "developers",
        "claude" => "ai",
        _ => "applications",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assigns_each_app_one_cleanup_owner() {
        assert_eq!(app_category("npm"), "developers");
        assert_eq!(app_category("cursor-partitions"), "developers");
        assert_eq!(app_category("claude"), "ai");
        assert_eq!(app_category("discord"), "applications");
    }
}
