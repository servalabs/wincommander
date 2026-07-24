use base64::Engine;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::process::Command;
use std::sync::Mutex;

const PROFILE_STORE: &str = "game-mode-profile-v1";
const GAME_BAR_PATH: &str = "Software\\Microsoft\\GameBar";
const GAME_MODE_VALUES: [&str; 2] = ["AutoGameModeEnabled", "AllowAutoGameMode"];
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

static PROFILE_MUTATION_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameModeValue {
    pub name: String,
    pub exists: bool,
    pub kind: Option<String>,
    pub value: Option<Value>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GameModeCache {
    key_exists: bool,
    values: Vec<GameModeValue>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameModePreview {
    pub profile_active: bool,
    pub current_values: Vec<GameModeValue>,
    pub changes: Vec<GameModeChange>,
    pub restore_available: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameModeChange {
    pub name: String,
    pub current_value: Option<Value>,
    pub desired_value: u32,
    pub will_change: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameModeOperation {
    pub action: String,
    pub preview: GameModePreview,
}
pub async fn game_mode_preview() -> Result<GameModePreview, String> {
    run_blocking(preview_profile).await
}
pub async fn game_mode_apply() -> Result<GameModeOperation, String> {
    run_blocking(apply_profile).await
}
pub async fn game_mode_restore() -> Result<GameModeOperation, String> {
    run_blocking(restore_profile).await
}
async fn run_blocking<T: Send + 'static>(work: fn() -> Result<T, String>) -> Result<T, String> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|error| format!("game mode task failed: {error}"))?
}
fn preview_profile() -> Result<GameModePreview, String> {
    let cache = load_cache()?;
    let current_values = read_game_bar_values()?;
    let changes = current_values
        .iter()
        .map(|value| GameModeChange {
            name: value.name.clone(),
            current_value: value.value.clone(),
            desired_value: 1,
            will_change: !value.exists || value.value.as_ref().and_then(Value::as_u64) != Some(1),
        })
        .collect();
    Ok(GameModePreview {
        profile_active: cache.is_some(),
        current_values,
        changes,
        restore_available: cache.is_some(),
    })
}

fn apply_profile() -> Result<GameModeOperation, String> {
    ensure_mutation_allowed()?;
    let _guard = PROFILE_MUTATION_LOCK
        .lock()
        .map_err(|_| "game mode lock poisoned".to_string())?;
    if load_cache()?.is_none() {
        let cache = GameModeCache {
            key_exists: game_bar_key_exists()?,
            values: read_game_bar_values()?,
        };
        save_cache(Some(&cache))?;
        if let Err(error) = set_game_mode_values() {
            let _ = restore_values(&cache);
            let _ = save_cache(None);
            return Err(error);
        }
    }
    Ok(GameModeOperation {
        action: "applied".into(),
        preview: preview_profile()?,
    })
}

fn restore_profile() -> Result<GameModeOperation, String> {
    ensure_mutation_allowed()?;
    let _guard = PROFILE_MUTATION_LOCK
        .lock()
        .map_err(|_| "game mode lock poisoned".to_string())?;
    let cache = load_cache()?.ok_or_else(|| "no Game Mode profile state is cached".to_string())?;
    restore_values(&cache)?;
    save_cache(None)?;
    Ok(GameModeOperation {
        action: "restored".into(),
        preview: preview_profile()?,
    })
}

fn ensure_mutation_allowed() -> Result<(), String> {
    if crate::settings::is_decoy_mode() {
        return Err("Refused: Game Mode changes are unavailable in Decoy mode.".into());
    }
    if crate::license::is_advanced_mode() {
        return Err("Refused: investigator mode forbids Game Mode changes because they would alter evidence.".into());
    }
    Ok(())
}

fn load_cache() -> Result<Option<GameModeCache>, String> {
    let stored = crate::datastore::load(PROFILE_STORE)?;
    if stored.as_object().is_none_or(|object| object.is_empty()) {
        return Ok(None);
    }
    let cache: GameModeCache = serde_json::from_value(stored)
        .map_err(|_| "cached Game Mode state is invalid; refusing mutation".to_string())?;
    if is_valid_cache(&cache) {
        Ok(Some(cache))
    } else {
        Err("cached Game Mode state is invalid; refusing mutation".into())
    }
}

fn save_cache(cache: Option<&GameModeCache>) -> Result<(), String> {
    crate::datastore::save(
        PROFILE_STORE,
        &cache.map_or_else(|| json!({}), |state| json!(state)),
    )
}

fn is_valid_cache(cache: &GameModeCache) -> bool {
    has_expected_game_mode_values(&cache.values) && cache.values.iter().all(valid_value)
}

fn has_expected_game_mode_values(values: &[GameModeValue]) -> bool {
    values.len() == GAME_MODE_VALUES.len()
        && GAME_MODE_VALUES
            .iter()
            .all(|name| values.iter().filter(|value| value.name == *name).count() == 1)
}

fn valid_value(value: &GameModeValue) -> bool {
    if !value.exists {
        return value.kind.is_none() && value.value.is_none();
    }
    matches!(
        (value.kind.as_deref(), value.value.as_ref()),
        (Some("DWord" | "QWord"), Some(Value::Number(_)))
            | (
                Some("String" | "ExpandString" | "Binary"),
                Some(Value::String(_))
            )
            | (Some("MultiString"), Some(Value::Array(_)))
    )
}

fn run_powershell(script: &str, state: Option<&GameModeCache>) -> Result<String, String> {
    let mut command = Command::new("powershell.exe");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command.args([
        "-NoLogo",
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        script,
    ]);
    if let Some(state) = state {
        let encoded = base64::engine::general_purpose::STANDARD
            .encode(serde_json::to_vec(state).map_err(|error| error.to_string())?);
        command.env("WINCOMMANDER_GAME_MODE_STATE", encoded);
    }
    let output = command
        .output()
        .map_err(|error| format!("Game Mode registry query failed: {error}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(format!(
            "Game Mode registry operation failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

fn game_bar_key_exists() -> Result<bool, String> {
    Ok(run_powershell(
        &format!(
            "$k=[Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('{GAME_BAR_PATH}',$false); [bool]$k"
        ),
        None,
    )? == "True")
}

fn read_game_bar_values() -> Result<Vec<GameModeValue>, String> {
    let calls = GAME_MODE_VALUES
        .iter()
        .map(|name| format!("Read-GameBarValue '{name}'"))
        .collect::<Vec<_>>()
        .join("; ");
    let script = format!(
        r#"
$k=[Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('{GAME_BAR_PATH}',$false)
function Read-GameBarValue([string]$n) {{
  if ($null -eq $k) {{ return [pscustomobject]@{{name=$n;exists=$false;kind=$null;value=$null}} }}
  try {{ $kind=$k.GetValueKind($n).ToString() }} catch {{ return [pscustomobject]@{{name=$n;exists=$false;kind=$null;value=$null}} }}
  $value=$k.GetValue($n,$null,[Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
  switch ($kind) {{ 'Binary' {{$value=[Convert]::ToBase64String([byte[]]$value)}} 'MultiString' {{$value=@([string[]]$value)}} }}
  [pscustomobject]@{{name=$n;exists=$true;kind=$kind;value=$value}}
}}
ConvertTo-Json -Compress -InputObject @({calls})
"#
    );
    parse_game_bar_values(&run_powershell(&script, None)?)
}

fn parse_game_bar_values(response: &str) -> Result<Vec<GameModeValue>, String> {
    let decoded: Value = serde_json::from_str(response)
        .map_err(|error| format!("invalid Game Mode registry response: {error}"))?;
    let values = match decoded {
        Value::Array(_) => serde_json::from_value(decoded),
        Value::Object(_) => serde_json::from_value(decoded).map(|value| vec![value]),
        _ => {
            return Err(
                "invalid Game Mode registry response: expected an object or array".to_string(),
            )
        }
    }
    .map_err(|error| format!("invalid Game Mode registry response: {error}"))?;

    if !has_expected_game_mode_values(&values) {
        return Err(format!(
            "invalid Game Mode registry response: expected exactly {}",
            GAME_MODE_VALUES.join(" and ")
        ));
    }
    if !values.iter().all(valid_value) {
        return Err("invalid Game Mode registry response: invalid value shape".to_string());
    }
    Ok(values)
}

fn set_game_mode_values() -> Result<(), String> {
    run_powershell(
        &format!(
            r#"
$k=[Microsoft.Win32.Registry]::CurrentUser.CreateSubKey('{GAME_BAR_PATH}')
$k.SetValue('AutoGameModeEnabled',1,[Microsoft.Win32.RegistryValueKind]::DWord)
$k.SetValue('AllowAutoGameMode',1,[Microsoft.Win32.RegistryValueKind]::DWord)
"#
        ),
        None,
    )
    .map(|_| ())
}

fn restore_values(cache: &GameModeCache) -> Result<(), String> {
    run_powershell(&format!(r#"
$s=[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($env:WINCOMMANDER_GAME_MODE_STATE)) | ConvertFrom-Json
$base=[Microsoft.Win32.Registry]::CurrentUser; $k=$base.OpenSubKey('{GAME_BAR_PATH}',$true)
foreach ($entry in @($s.values)) {{
  if ($entry.exists) {{
    if ($null -eq $k) {{$k=$base.CreateSubKey('{GAME_BAR_PATH}')}}
    $kind=[Enum]::Parse([Microsoft.Win32.RegistryValueKind],[string]$entry.kind)
    $value=switch ([string]$entry.kind) {{ 'DWord' {{[int]$entry.value}} 'QWord' {{[int64]$entry.value}} 'Binary' {{[Convert]::FromBase64String([string]$entry.value)}} 'MultiString' {{[string[]]@($entry.value)}} default {{[string]$entry.value}} }}
    $k.SetValue([string]$entry.name,$value,$kind)
  }} elseif ($null -ne $k) {{$k.DeleteValue([string]$entry.name,$false)}}
}}
if (-not $s.keyExists -and $null -ne $k -and @($k.GetValueNames()).Count -eq 0 -and @($k.GetSubKeyNames()).Count -eq 0) {{$k.Close();$base.DeleteSubKey('{GAME_BAR_PATH}',$false)}}
"#), Some(cache)).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_array_response_with_both_expected_values() {
        let response = r#"[
            {"name":"AutoGameModeEnabled","exists":false,"kind":null,"value":null},
            {"name":"AllowAutoGameMode","exists":true,"kind":"DWord","value":1}
        ]"#;

        let values = parse_game_bar_values(response).expect("valid registry response");

        assert_eq!(values.len(), 2);
        assert_eq!(values[0].name, "AutoGameModeEnabled");
        assert_eq!(values[1].name, "AllowAutoGameMode");
    }

    #[test]
    fn normalizes_object_response_before_rejecting_missing_expected_value() {
        let response = r#"{"name":"AutoGameModeEnabled","exists":false,"kind":null,"value":null}"#;

        let error = parse_game_bar_values(response)
            .err()
            .expect("one value must fail the two-name invariant");

        assert!(error.contains("expected exactly"));
        assert!(!error.contains("expected a sequence"));
    }

    #[test]
    fn rejects_duplicate_or_unexpected_names() {
        let response = r#"[
            {"name":"AutoGameModeEnabled","exists":false,"kind":null,"value":null},
            {"name":"AutoGameModeEnabled","exists":false,"kind":null,"value":null}
        ]"#;

        let error = parse_game_bar_values(response)
            .err()
            .expect("duplicate names must be rejected");

        assert!(error.contains("expected exactly"));
    }

    #[test]
    fn rejects_invalid_top_level_shape() {
        let error = parse_game_bar_values("true")
            .err()
            .expect("scalar response must be rejected");

        assert!(error.contains("expected an object or array"));
    }

    #[test]
    fn rejects_invalid_registry_value_shape() {
        let response = r#"[
            {"name":"AutoGameModeEnabled","exists":false,"kind":"DWord","value":null},
            {"name":"AllowAutoGameMode","exists":true,"kind":"DWord","value":1}
        ]"#;

        let error = parse_game_bar_values(response)
            .err()
            .expect("inconsistent exists metadata must be rejected");

        assert!(error.contains("invalid value shape"));
    }

    #[test]
    fn only_the_two_game_bar_values_are_allowed_in_the_cache() {
        let cache = GameModeCache {
            key_exists: false,
            values: GAME_MODE_VALUES.into_iter().map(missing_value).collect(),
        };
        assert!(is_valid_cache(&cache));
        let mut invalid = cache;
        invalid.values[0].name = "OtherSetting".into();
        assert!(!is_valid_cache(&invalid));
    }

    fn missing_value(name: &str) -> GameModeValue {
        GameModeValue {
            name: name.into(),
            exists: false,
            kind: None,
            value: None,
        }
    }
}
