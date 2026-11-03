// SPDX-License-Identifier: AGPL-3.0-or-later
use crate::{BuiltinPattern, DetectorCategory};
pub(crate) const LEGACY: &[BuiltinPattern] = &[
    BuiltinPattern::AwsAccessKey,
    BuiltinPattern::GoogleApiKey,
    BuiltinPattern::SendgridApiKey,
    BuiltinPattern::MailgunApiKey,
    BuiltinPattern::TwilioAccountSid,
    BuiltinPattern::DatabaseUrlWithCredentials,
    BuiltinPattern::OpenAiProjectKey,
    BuiltinPattern::OpenAiApiKey,
    BuiltinPattern::AnthropicApiKey,
    BuiltinPattern::GitHubClassicToken,
    BuiltinPattern::GitHubFineGrainedToken,
    BuiltinPattern::NpmToken,
    BuiltinPattern::StripeLiveSecret,
    BuiltinPattern::StripeLivePublishable,
    BuiltinPattern::SlackToken,
    BuiltinPattern::DiscordBotToken,
    BuiltinPattern::PrivateKeyPem,
    BuiltinPattern::SshPrivateKeyHeader,
    BuiltinPattern::Jwt,
    BuiltinPattern::BitcoinWifPrivateKey,
    BuiltinPattern::PowershellEncodedPayload,
    BuiltinPattern::HiddenPowershellWindow,
    BuiltinPattern::PowershellExecutionPolicyBypass,
    BuiltinPattern::PowershellRemoteDownloadExecute,
    BuiltinPattern::MshtaWebPayload,
    BuiltinPattern::CertutilWebDownload,
    BuiltinPattern::Regsvr32WebPayload,
    BuiltinPattern::BitsadminWebTransfer,
    BuiltinPattern::CurlWgetPipeToShell,
    BuiltinPattern::UnicodeBidiOverride,
    BuiltinPattern::UnicodeZeroWidthInCode,
    BuiltinPattern::UnicodeConfusableUrlHost,
];

pub(crate) fn legacy_id(pattern: BuiltinPattern) -> String {
    serde_json::to_value(pattern)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_else(|| "custom".into())
}
pub(crate) fn legacy_category(pattern: BuiltinPattern) -> DetectorCategory {
    use BuiltinPattern::*;
    match pattern {
        AwsAccessKey | GoogleApiKey | SendgridApiKey | MailgunApiKey | TwilioAccountSid => {
            DetectorCategory::CloudKey
        }
        DatabaseUrlWithCredentials => DetectorCategory::ConnectionString,
        PrivateKeyPem | SshPrivateKeyHeader => DetectorCategory::PrivateKey,
        BitcoinWifPrivateKey => DetectorCategory::Crypto,
        PowershellEncodedPayload
        | HiddenPowershellWindow
        | PowershellExecutionPolicyBypass
        | PowershellRemoteDownloadExecute
        | MshtaWebPayload
        | CertutilWebDownload
        | Regsvr32WebPayload
        | BitsadminWebTransfer
        | CurlWgetPipeToShell
        | UnicodeBidiOverride
        | UnicodeZeroWidthInCode
        | UnicodeConfusableUrlHost => DetectorCategory::Custom,
        _ => DetectorCategory::Token,
    }
}
pub(crate) fn legacy_keywords(pattern: BuiltinPattern) -> &'static [&'static str] {
    use BuiltinPattern::*;
    match pattern {
        AwsAccessKey => &["AKIA"],
        GoogleApiKey => &["AIza"],
        SendgridApiKey => &["SG."],
        MailgunApiKey => &["key-"],
        TwilioAccountSid => &["AC"],
        DatabaseUrlWithCredentials => &["://"],
        OpenAiProjectKey => &["sk-proj-"],
        OpenAiApiKey => &["sk-"],
        AnthropicApiKey => &["sk-ant-"],
        GitHubClassicToken => &["ghp_", "gho_", "ghu_", "ghs_", "ghr_"],
        GitHubFineGrainedToken => &["github_pat_"],
        NpmToken => &["npm_"],
        StripeLiveSecret => &["sk_live_"],
        StripeLivePublishable => &["pk_live_"],
        SlackToken => &["xox"],
        PrivateKeyPem | SshPrivateKeyHeader => &["PRIVATE KEY"],
        Jwt => &["eyJ"],
        PowershellEncodedPayload | HiddenPowershellWindow | PowershellExecutionPolicyBypass => {
            &["powershell"]
        }
        PowershellRemoteDownloadExecute => &["iex", "invoke-expression"],
        MshtaWebPayload => &["mshta"],
        CertutilWebDownload => &["certutil"],
        Regsvr32WebPayload => &["regsvr32"],
        BitsadminWebTransfer => &["bitsadmin"],
        CurlWgetPipeToShell => &[
            "curl",
            "wget",
            "iwr",
            "irm",
            "invoke-webrequest",
            "invoke-restmethod",
        ],
        _ => &[],
    }
}
