// SPDX-License-Identifier: AGPL-3.0-or-later
//! Headless, machine-readable WinCommander command surface.
//!
//! The catalog is generated from the desktop handler registry, backend
//! dispatcher, and frontend call sites. Backend-script commands execute through
//! the same `run_backend_script` function used by the GUI, preserving licence,
//! module, administrator, investigator-mode, and Pro-sidecar enforcement.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::Read;
use std::sync::{
    atomic::{AtomicBool, AtomicI32, Ordering},
    Arc, LazyLock, Mutex,
};

const CATALOG_JSON: &str = include_str!("cli_catalog.generated.json");

pub fn is_cli_invocation(args: &[String]) -> bool {
    matches!(
        args.first().map(String::as_str),
        Some("commands" | "audit" | "run" | "help" | "--help" | "-h")
    )
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeneratedCatalog {
    schema_version: u32,
    commands: Vec<GeneratedCommand>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeneratedCommand {
    id: String,
    name: String,
    transport: Transport,
    registered: bool,
    /// Emitted by the generator from the `#[cfg(debug_assertions)]` attribute on
    /// the handler's `generate_handler!` entry. Deliberately not `serde(default)`:
    /// a catalog that predates the field must fail loudly rather than silently
    /// report every debug-gated handler as executable in a release build.
    debug_only: bool,
    frontend_references: Vec<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum Transport {
    Tauri,
    BackendScript,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum Risk {
    ReadOnly,
    Mutating,
    Destructive,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum HeadlessSupport {
    Executable,
    UiOnly,
    Cataloged,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CommandDescription {
    id: String,
    name: String,
    transport: Transport,
    registered: bool,
    debug_only: bool,
    frontend_references: Vec<String>,
    tier: String,
    risk: Risk,
    headless_support: HeadlessSupport,
    confirmation: Option<String>,
}

#[derive(Debug)]
struct RunRequest {
    id: String,
    params: Value,
    dry_run: bool,
    confirmation: Option<String>,
    timeout_ms: u64,
    timeout_explicit: bool,
}

#[derive(Clone, Debug)]
struct TauriRunSpec {
    id: String,
    command: String,
    params: Value,
    timeout_ms: Option<u64>,
}

static TAURI_RUN: LazyLock<Mutex<Option<TauriRunSpec>>> = LazyLock::new(|| Mutex::new(None));
static TAURI_RUN_ACTIVE: AtomicBool = AtomicBool::new(false);
static TAURI_RUN_BRIDGE_READY: AtomicBool = AtomicBool::new(false);
static TAURI_RUN_EXIT_CODE: AtomicI32 = AtomicI32::new(9);
/// Claimed by whichever of the backend dispatcher and its wait deadline
/// finishes first, so exactly one JSON document reaches stdout.
/// The same guarantee for the native path, where a terminal command's
/// acknowledgement is emitted up front and both watchdogs can still fire.
static TAURI_RESULT_EMITTED: AtomicBool = AtomicBool::new(false);

fn spawn_backend_deadline(
    timeout_ms: u64,
    settled: Arc<AtomicBool>,
    on_timeout: impl FnOnce() + Send + 'static,
) {
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(timeout_ms));
        if !settled.swap(true, Ordering::SeqCst) {
            on_timeout();
        }
    });
}

struct CliExecutionLock {
    #[cfg(windows)]
    handle: isize,
}

impl CliExecutionLock {
    #[cfg(windows)]
    fn acquire() -> Result<Self, String> {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{CreateMutexW, WaitForSingleObject};

        const WAIT_OBJECT_0: u32 = 0;
        const WAIT_ABANDONED: u32 = 0x80;
        const WAIT_TIMEOUT: u32 = 0x102;
        let name: Vec<u16> = "WinCommander_CliExecution_lock\0".encode_utf16().collect();
        let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
        if handle.is_null() {
            return Err("failed to create the cross-process CLI execution lock".to_string());
        }
        match unsafe { WaitForSingleObject(handle, 30_000) } {
            WAIT_OBJECT_0 | WAIT_ABANDONED => Ok(Self {
                handle: handle as isize,
            }),
            WAIT_TIMEOUT => {
                unsafe { CloseHandle(handle) };
                Err("another mutating WinCommander CLI command is still running".to_string())
            }
            _ => {
                unsafe { CloseHandle(handle) };
                Err("failed to acquire the cross-process CLI execution lock".to_string())
            }
        }
    }

    #[cfg(not(windows))]
    fn acquire() -> Result<Self, String> {
        Ok(Self {})
    }
}

impl Drop for CliExecutionLock {
    fn drop(&mut self) {
        #[cfg(windows)]
        if self.handle != 0 {
            use windows_sys::Win32::Foundation::CloseHandle;
            use windows_sys::Win32::System::Threading::ReleaseMutex;
            unsafe {
                ReleaseMutex(self.handle as _);
                CloseHandle(self.handle as _);
            }
        }
    }
}

pub fn main(args: Vec<String>) -> i32 {
    crate::backend::register_p2_commands();
    crate::backend::register_p3_commands();
    crate::backend::register_file_search_commands();

    match parse_action(&args) {
        Ok(Action::Help) => {
            print_json(&json!({
                "ok": true,
                "usage": [
                    "wincommander-free commands list [--transport tauri|backend-script] [--risk read-only|mutating|destructive]",
                    "wincommander-free commands describe <tauri:name|backend:name>",
                    "wincommander-free run <backend:name|tauri:name> [--params <json|@file|->] [--dry-run] [--confirm <token>] [--timeout-ms <milliseconds> (read-only only)]",
                    "wincommander-free audit catalog"
                ],
                "output": "JSON on stdout; process exit code reports success or failure"
            }));
            0
        }
        Ok(Action::List { transport, risk }) => list_commands(transport, risk),
        Ok(Action::Describe(id)) => describe_command(&id),
        Ok(Action::AuditCatalog) => audit_catalog(),
        Ok(Action::Run(request)) => run_command(request),
        Err(error) => fail(2, "invalid_arguments", error),
    }
}

enum Action {
    Help,
    List {
        transport: Option<Transport>,
        risk: Option<Risk>,
    },
    Describe(String),
    AuditCatalog,
    Run(RunRequest),
}

fn parse_action(args: &[String]) -> Result<Action, String> {
    let filtered: Vec<&str> = args
        .iter()
        .map(String::as_str)
        .filter(|arg| *arg != "--json")
        .collect();
    if filtered.is_empty() || matches!(filtered[0], "help" | "--help" | "-h") {
        return Ok(Action::Help);
    }
    match filtered.as_slice() {
        ["commands", "list", rest @ ..] => {
            let mut transport = None;
            let mut risk = None;
            let mut index = 0;
            while index < rest.len() {
                match rest[index] {
                    "--transport" => {
                        let value = rest.get(index + 1).ok_or("--transport requires a value")?;
                        transport = Some(parse_transport(value)?);
                        index += 2;
                    }
                    "--risk" => {
                        let value = rest.get(index + 1).ok_or("--risk requires a value")?;
                        risk = Some(parse_risk(value)?);
                        index += 2;
                    }
                    other => return Err(format!("unknown commands list argument: {other}")),
                }
            }
            Ok(Action::List { transport, risk })
        }
        ["commands", "describe", id] => Ok(Action::Describe((*id).to_string())),
        ["audit", "catalog"] => Ok(Action::AuditCatalog),
        ["run", id, rest @ ..] => {
            let mut params = json!({});
            let mut dry_run = false;
            let mut confirmation = None;
            let mut timeout_ms = 300_000u64;
            let mut timeout_explicit = false;
            let mut index = 0;
            while index < rest.len() {
                match rest[index] {
                    "--params" => {
                        let value = option_value(rest, index, "--params")?;
                        params = parse_params(value)?;
                        index += 2;
                    }
                    "--dry-run" => {
                        dry_run = true;
                        index += 1;
                    }
                    "--confirm" => {
                        let value = option_value(rest, index, "--confirm")?;
                        confirmation = Some((*value).to_string());
                        index += 2;
                    }
                    "--timeout-ms" => {
                        let value = option_value(rest, index, "--timeout-ms")?;
                        timeout_ms = value
                            .parse::<u64>()
                            .map_err(|_| "--timeout-ms must be an integer")?;
                        if !(100..=3_600_000).contains(&timeout_ms) {
                            return Err("--timeout-ms must be between 100 and 3600000".to_string());
                        }
                        timeout_explicit = true;
                        index += 2;
                    }
                    other => return Err(format!("unknown run argument: {other}")),
                }
            }
            Ok(Action::Run(RunRequest {
                id: (*id).to_string(),
                params,
                dry_run,
                confirmation,
                timeout_ms,
                timeout_explicit,
            }))
        }
        _ => Err("unknown command; run `wincommander-cli help`".to_string()),
    }
}

/// Reads the value that follows an option, refusing anything flag-shaped.
///
/// Silently swallowing the next token is not a cosmetic parser flaw here:
/// `run <read-only-id> --confirm --dry-run` used to absorb `--dry-run` as the
/// confirmation string, leaving `dry_run` false. A read-only command needs no
/// confirmation, so the bogus value was never checked either — the operator
/// asked for a preview and got a live execution. The same swallow could smuggle
/// `--safe-copy` past this parser and into the argv scan in `lib.rs::run`.
fn option_value<'a>(rest: &'a [&'a str], index: usize, flag: &str) -> Result<&'a str, String> {
    match rest.get(index + 1) {
        None => Err(format!("{flag} requires a value")),
        Some(value) if value.starts_with("--") => Err(format!(
            "{flag} requires a value, but found the flag {value}"
        )),
        Some(value) => Ok(*value),
    }
}

fn parse_transport(value: &str) -> Result<Transport, String> {
    match value {
        "tauri" => Ok(Transport::Tauri),
        "backend-script" | "backend" => Ok(Transport::BackendScript),
        _ => Err(format!("unknown transport: {value}")),
    }
}

fn parse_risk(value: &str) -> Result<Risk, String> {
    match value {
        "read-only" | "readonly" => Ok(Risk::ReadOnly),
        "mutating" => Ok(Risk::Mutating),
        "destructive" => Ok(Risk::Destructive),
        _ => Err(format!("unknown risk: {value}")),
    }
}

fn parse_params(spec: &str) -> Result<Value, String> {
    let raw = if spec == "-" {
        let mut input = String::new();
        std::io::stdin()
            .read_to_string(&mut input)
            .map_err(|error| format!("failed to read params from stdin: {error}"))?;
        input
    } else if let Some(path) = spec.strip_prefix('@') {
        std::fs::read_to_string(path)
            .map_err(|error| format!("failed to read params file '{path}': {error}"))?
    } else {
        spec.to_string()
    };
    let value: Value = serde_json::from_str(&raw)
        .map_err(|error| format!("params must be a JSON object: {error}"))?;
    if !value.is_object() {
        return Err("params must be a JSON object".to_string());
    }
    Ok(value)
}

fn backend_params(value: &Value) -> Result<HashMap<String, String>, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "params must be a JSON object".to_string())?;
    Ok(object
        .iter()
        .map(|(key, value)| {
            let value = value
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| value.to_string());
            (key.clone(), value)
        })
        .collect())
}

fn catalog() -> Result<GeneratedCatalog, String> {
    serde_json::from_str(CATALOG_JSON)
        .map_err(|error| format!("embedded catalog is invalid: {error}"))
}

fn describe(entry: &GeneratedCommand) -> CommandDescription {
    let risk = classify_risk(entry);
    let headless_support = classify_headless_support(entry);
    let tier = match entry.transport {
        Transport::BackendScript => crate::backend::get_command_tier(&entry.name).to_string(),
        Transport::Tauri => "handler-enforced".to_string(),
    };
    let confirmation = match risk {
        Risk::ReadOnly => None,
        Risk::Mutating => Some(format!("RUN:{}", entry.id)),
        Risk::Destructive => Some(format!("DESTROY:{}", entry.id)),
    };
    CommandDescription {
        id: entry.id.clone(),
        name: entry.name.clone(),
        transport: entry.transport,
        registered: available_in_this_build(entry),
        debug_only: entry.debug_only,
        frontend_references: entry.frontend_references.clone(),
        tier,
        risk,
        headless_support,
        confirmation,
    }
}

fn available_in_this_build(entry: &GeneratedCommand) -> bool {
    entry.registered && (cfg!(debug_assertions) || !entry.debug_only)
}

/// Handlers whose name misreports its effect, verified against the handler body
/// rather than its spelling. Without these each one would be granted a weaker
/// confirmation token than the work it performs — and a read-only
/// classification also makes a command eligible for the wait deadline, so an
/// export could be killed part-written.
const RISK_OVERRIDES: &[(&str, Risk)] = &[
    // `search_` reads, but these two rename a file on disk and overwrite the
    // clipboard respectively.
    ("tauri:search_rename_file", Risk::Mutating),
    ("tauri:search_set_file_clipboard", Risk::Mutating),
    // `export_` writes a new artefact.
    ("tauri:export_evidence_vault", Risk::Mutating),
    ("tauri:export_evidence_affidavit", Risk::Mutating),
    ("tauri:export_flow_bundle", Risk::Mutating),
    ("tauri:export_settings_cmd", Risk::Mutating),
    // `_snapshot` reads elsewhere; this one pushes posture to the fleet server.
    ("tauri:fleet_update_posture_snapshot", Risk::Mutating),
];

/// Words that only ever denote irreversible destruction of user data. Matched
/// anywhere in the name, because the destructive prefix lists are anchored at
/// the start and so miss `Invoke-7Erase`, `Invoke-UnallocatedSpaceErase`, and
/// the rest of the `Invoke-*Erase` family. Applied only after the read-only
/// allowlist has had its say, so `Get-AutoEraseSchedules` stays a read.
const ERASURE_WORDS: &[&str] = &["erase", "shred", "wipe", "destroy"];

/// Fails closed, in priority order: the repository's authoritative destructive
/// registry outranks every heuristic, then hand-verified overrides, then the
/// name rules. `authz::DESTRUCTIVE_COMMANDS` is the same list the CI
/// authorization gate enforces, so a newly registered catastrophic command
/// demands `DESTROY:` here the moment it is added there — it no longer depends
/// on someone also picking a scary-enough name.
fn classify_risk(entry: &GeneratedCommand) -> Risk {
    if entry.transport == Transport::Tauri && crate::authz::action_for(&entry.name).is_some() {
        return Risk::Destructive;
    }
    if let Some((_, risk)) = RISK_OVERRIDES.iter().find(|(id, _)| *id == entry.id) {
        return *risk;
    }
    let name = entry.name.to_ascii_lowercase();
    let destructive = match entry.transport {
        Transport::BackendScript => {
            [
                "clear-", "delete-", "remove-", "erase-", "destroy-", "shred-", "wipe-",
            ]
            .iter()
            .any(|prefix| name.starts_with(prefix))
                || name.starts_with("invoke-cleanup")
                || name.starts_with("format-")
                || name.contains("ssdtrim")
        }
        Transport::Tauri => {
            [
                "clear", "delete", "remove", "erase", "destroy", "shred", "wipe", "clean", "format",
            ]
            .iter()
            .any(|word| name.split('_').any(|token| token == *word))
                || matches!(
                    name.as_str(),
                    "lockdown"
                        | "full_lockdown"
                        | "fire_flow"
                        | "run_backend_script"
                        | "test_pro_dispatch"
                )
        }
    };
    if destructive {
        return Risk::Destructive;
    }
    let read_only = match entry.transport {
        Transport::BackendScript => [
            "get-", "test-", "find-", "scan-", "search-", "measure-", "compare-",
        ]
        .iter()
        .any(|prefix| name.starts_with(prefix)),
        Transport::Tauri => {
            [
                "get_", "list_", "is_", "search_", "read_", "export_", "check_", "scan_",
            ]
            .iter()
            .any(|prefix| name.starts_with(prefix))
                || name.ends_with("_status")
                || name.ends_with("_preview")
                || name.ends_with("_inventory")
                || name.ends_with("_snapshot")
        }
    };
    if read_only {
        return Risk::ReadOnly;
    }
    if ERASURE_WORDS.iter().any(|word| name.contains(word)) {
        return Risk::Destructive;
    }
    Risk::Mutating
}

fn classify_headless_support(entry: &GeneratedCommand) -> HeadlessSupport {
    if available_in_this_build(entry) && classify_risk(entry) == Risk::ReadOnly {
        return HeadlessSupport::Executable;
    }
    let name = entry.name.as_str();
    if name.starts_with("open_")
        || name.starts_with("show_")
        || name.contains("server_app")
        || name.contains("calculator_mode")
        || name.contains("display_label")
        || name.contains("tray_")
    {
        HeadlessSupport::UiOnly
    } else if available_in_this_build(entry) {
        // Mutating and destructive handlers remain desktop-only until every
        // command has the native capability gate and shared cross-process locks.
        HeadlessSupport::UiOnly
    } else {
        HeadlessSupport::Cataloged
    }
}

fn list_commands(transport: Option<Transport>, risk: Option<Risk>) -> i32 {
    let catalog = match catalog() {
        Ok(catalog) => catalog,
        Err(error) => return fail(5, "catalog_error", error),
    };
    let commands: Vec<CommandDescription> = catalog
        .commands
        .iter()
        .map(describe)
        .filter(|entry| {
            transport
                .map(|value| entry.transport == value)
                .unwrap_or(true)
        })
        .filter(|entry| risk.map(|value| entry.risk == value).unwrap_or(true))
        .collect();
    print_json(
        &json!({ "ok": true, "schemaVersion": catalog.schema_version, "count": commands.len(), "commands": commands }),
    );
    0
}

fn describe_command(id: &str) -> i32 {
    let catalog = match catalog() {
        Ok(catalog) => catalog,
        Err(error) => return fail(5, "catalog_error", error),
    };
    match catalog.commands.iter().find(|entry| entry.id == id) {
        Some(entry) => {
            print_json(&json!({ "ok": true, "command": describe(entry) }));
            0
        }
        None => fail(4, "unknown_command", format!("command not found: {id}")),
    }
}

fn audit_catalog() -> i32 {
    let catalog = match catalog() {
        Ok(catalog) => catalog,
        Err(error) => return fail(5, "catalog_error", error),
    };
    let descriptions: Vec<CommandDescription> = catalog.commands.iter().map(describe).collect();
    let missing: Vec<&CommandDescription> = descriptions
        .iter()
        .filter(|entry| {
            !entry.registered && !entry.debug_only && !entry.frontend_references.is_empty()
        })
        .collect();
    let missing_headless: Vec<&CommandDescription> = descriptions
        .iter()
        .filter(|entry| entry.registered && entry.headless_support != HeadlessSupport::Executable)
        .collect();
    let count = |risk| {
        descriptions
            .iter()
            .filter(|entry| entry.risk == risk)
            .count()
    };
    print_json(&json!({
        "ok": missing.is_empty() && missing_headless.is_empty(),
        "schemaVersion": catalog.schema_version,
        "total": descriptions.len(),
        "transports": {
            "tauri": descriptions.iter().filter(|entry| entry.transport == Transport::Tauri).count(),
            "backendScript": descriptions.iter().filter(|entry| entry.transport == Transport::BackendScript).count()
        },
        "risk": {
            "readOnly": count(Risk::ReadOnly),
            "mutating": count(Risk::Mutating),
            "destructive": count(Risk::Destructive)
        },
        "headless": {
            "executable": descriptions.iter().filter(|entry| entry.headless_support == HeadlessSupport::Executable).count(),
            "uiOnly": descriptions.iter().filter(|entry| entry.headless_support == HeadlessSupport::UiOnly).count(),
            "cataloged": descriptions.iter().filter(|entry| entry.headless_support == HeadlessSupport::Cataloged).count()
        },
        "missingDispatchers": missing,
        "missingHeadlessAdapters": missing_headless
    }));
    if missing.is_empty() && missing_headless.is_empty() {
        0
    } else {
        6
    }
}

fn run_command(request: RunRequest) -> i32 {
    let catalog = match catalog() {
        Ok(catalog) => catalog,
        Err(error) => return fail(5, "catalog_error", error),
    };
    let Some(entry) = catalog.commands.iter().find(|entry| entry.id == request.id) else {
        return fail(
            4,
            "unknown_command",
            format!("command not found: {}", request.id),
        );
    };
    let description = describe(entry);
    if request.dry_run {
        print_json(&json!({
            "ok": true,
            "dryRun": true,
            "command": description,
            "params": request.params,
            "wouldExecute": description.headless_support == HeadlessSupport::Executable
        }));
        return 0;
    }
    if description.headless_support != HeadlessSupport::Executable {
        return fail(
            7,
            "headless_not_enabled",
            format!(
                "{} is cataloged but does not yet have a shared headless adapter",
                description.id
            ),
        );
    }
    if description.risk != Risk::ReadOnly && request.timeout_explicit {
        return fail(
            2,
            "invalid_arguments",
            "--timeout-ms is only supported for read-only commands; mutating and destructive commands run to completion"
                .to_string(),
        );
    }
    if let Some(expected) = &description.confirmation {
        if request.confirmation.as_deref() != Some(expected.as_str()) {
            print_json(&json!({
                "ok": false,
                "error": "confirmation_required",
                "command": description.id,
                "risk": description.risk,
                "expectedConfirmation": expected
            }));
            return 3;
        }
    }
    let _execution_lock = if description.risk == Risk::ReadOnly {
        None
    } else {
        match CliExecutionLock::acquire() {
            Ok(lock) => Some(lock),
            Err(error) => return fail(11, "cli_busy", error),
        }
    };
    // The desktop handler terminates its event loop before an IPC response can
    // be delivered. In a one-shot CLI process, successful return is itself the
    // requested exit operation, so emit the machine-readable acknowledgement
    // directly instead of starting a runtime that cannot reply.
    if entry.transport == Transport::Tauri && entry.name == "exit_app" {
        print_json(&json!({
            "ok": true,
            "command": description.id,
            "result": { "exited": true }
        }));
        return 0;
    }
    // `lockdown` and `full_lockdown` launch their cleanup worker and then tear
    // the process down, so no IPC completion ever arrives — and Tauri's
    // `App::run` exits the process directly rather than returning, so nothing
    // after `crate::run()` executes either. Emit the acknowledgement here,
    // while stdout is still certainly ours, or automation gets no JSON at all.
    // It acknowledges the dispatch, never the outcome: verify the wipe
    // independently.
    if entry.transport == Transport::Tauri && is_terminal_tauri_command(&entry.name) {
        TAURI_RESULT_EMITTED.store(true, Ordering::SeqCst);
        print_json(&json!({
            "ok": true,
            "command": description.id,
            "result": { "detached": true, "processExitRequested": true }
        }));
        use std::io::Write;
        let _ = std::io::stdout().flush();
    }
    let wait_deadline = (description.risk == Risk::ReadOnly).then_some(request.timeout_ms);
    match entry.transport {
        Transport::BackendScript => match backend_params(&request.params) {
            Ok(params) => run_backend(entry.name.clone(), params, wait_deadline),
            Err(error) => fail(2, "invalid_arguments", error),
        },
        Transport::Tauri => run_tauri(
            description.id,
            entry.name.clone(),
            request.params,
            wait_deadline,
        ),
    }
}

fn run_backend(command: String, params: HashMap<String, String>, timeout_ms: Option<u64>) -> i32 {
    let command_id = format!("backend:{command}");
    let output_id = command_id.clone();
    let execution_code = Arc::new(AtomicI32::new(9));
    let result_code = execution_code.clone();
    let settled = Arc::new(AtomicBool::new(false));
    if let Some(timeout_ms) = timeout_ms {
        let timeout_id = command_id.clone();
        let timeout_settled = settled.clone();
        // Tauri's async runtime does not exist until setup begins. A failed or
        // wedged runtime startup therefore needs an ordinary process thread;
        // starting the timer inside setup leaves automation hung indefinitely.
        spawn_backend_deadline(timeout_ms, timeout_settled, move || {
            print_json(&json!({
                "ok": false,
                "command": timeout_id,
                "error": "timeout",
                "message": format!("read-only command exceeded {timeout_ms} ms; this is a wait limit, not transactional cancellation")
            }));
            use std::io::Write;
            let _ = std::io::stdout().flush();
            std::process::exit(10);
        });
    }
    let mut context = tauri::generate_context!();
    context.config_mut().app.windows.clear();
    context.config_mut().build.dev_url = None;
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(move |app| {
            let app_handle = app.handle().clone();
            let result_handle = app_handle.clone();
            let command = command.clone();
            let command_id = output_id.clone();
            let execution_code = execution_code.clone();
            let settled = settled.clone();
            tauri::async_runtime::spawn(async move {
                let response = crate::backend::run_backend_script(result_handle.clone(), command, params).await;
                if settled.swap(true, Ordering::SeqCst) {
                    return;
                }
                match response {
                    Ok(result) => {
                        print_json(&json!({ "ok": true, "command": command_id, "result": result }));
                        execution_code.store(0, Ordering::SeqCst);
                        result_handle.exit(0);
                    }
                    Err(error) => {
                        print_json(&json!({ "ok": false, "command": command_id, "error": "execution_failed", "message": error }));
                        execution_code.store(8, Ordering::SeqCst);
                        result_handle.exit(8);
                    }
                }
            });
            Ok(())
        });
    match builder.run(context) {
        Ok(()) => result_code.load(Ordering::SeqCst),
        Err(error) => fail(
            9,
            "runtime_error",
            format!("failed to run {command_id}: {error}"),
        ),
    }
}

fn run_tauri(id: String, command: String, params: Value, timeout_ms: Option<u64>) -> i32 {
    match TAURI_RUN.lock() {
        Ok(mut slot) if slot.is_none() => {
            *slot = Some(TauriRunSpec {
                id,
                command,
                params,
                timeout_ms,
            });
        }
        Ok(_) => {
            return fail(
                9,
                "runtime_error",
                "a Tauri CLI command is already active".into(),
            )
        }
        Err(_) => return fail(9, "runtime_error", "Tauri CLI state is poisoned".into()),
    }
    TAURI_RUN_EXIT_CODE.store(9, Ordering::SeqCst);
    TAURI_RUN_BRIDGE_READY.store(false, Ordering::SeqCst);
    TAURI_RUN_ACTIVE.store(true, Ordering::SeqCst);
    crate::run();
    // Tauri's `App::run` exits the process directly and never returns, so this
    // is reached only if that contract changes. A terminal command has already
    // printed its acknowledgement before dispatch, hence the emitted-guard.
    if TAURI_RUN_ACTIVE.swap(false, Ordering::SeqCst)
        && !TAURI_RESULT_EMITTED.swap(true, Ordering::SeqCst)
    {
        return fail(
            9,
            "runtime_exited",
            "the Tauri runtime exited before the command returned".into(),
        );
    }
    TAURI_RUN_EXIT_CODE.load(Ordering::SeqCst)
}

fn is_terminal_tauri_command(command: &str) -> bool {
    matches!(command, "lockdown" | "full_lockdown")
}

pub(crate) fn tauri_runtime_active() -> bool {
    TAURI_RUN_ACTIVE.load(Ordering::SeqCst)
}

pub(crate) fn tauri_initialization_script() -> Result<String, String> {
    let spec = TAURI_RUN
        .lock()
        .map_err(|_| "Tauri CLI state is poisoned".to_string())?
        .clone()
        .ok_or_else(|| "Tauri CLI request is unavailable".to_string())?;
    let command = serde_json::to_string(&spec.command)
        .map_err(|error| format!("failed to encode command: {error}"))?;
    let params = serde_json::to_string(&spec.params)
        .map_err(|error| format!("failed to encode params: {error}"))?;
    // Encoded a second time so the JSON document arrives as a JS *string*
    // literal for JSON.parse, rather than as inline object syntax.
    let params_json = serde_json::to_string(&params)
        .map_err(|error| format!("failed to encode params: {error}"))?;
    Ok(format!(
        r#"
(() => {{
  if (window.__WINCOMMANDER_CLI_STARTED__) return;
  window.__WINCOMMANDER_CLI_STARTED__ = true;
  const command = {command};
  // Parsed, not spliced as an object literal: a literal would let a key named
  // __proto__ set the prototype instead of becoming an ordinary parameter, so
  // the handler would silently receive different params than were requested.
  const params = JSON.parse({params_json});
  const normalize = (value) => {{
    if (value == null) return null;
    if (value instanceof Error) return {{ name: value.name, message: value.message, stack: value.stack }};
    try {{ JSON.stringify(value); return value; }} catch (_) {{ return String(value); }}
  }};
  const waitForInvoke = async () => {{
    for (let attempt = 0; attempt < 500; attempt += 1) {{
      const invoke = window.__TAURI_INTERNALS__?.invoke;
      if (typeof invoke === "function") return invoke;
      await new Promise((resolve) => setTimeout(resolve, 10));
    }}
    throw new Error("Tauri invoke bridge did not initialize");
  }};
  void (async () => {{
    let invoke = null;
    try {{
      invoke = await waitForInvoke();
      await invoke("mark_tauri_cli_ready");
      const result = await invoke(command, params);
      await invoke("complete_tauri_cli", {{ ok: true, payload: normalize(result) }});
    }} catch (error) {{
      try {{
        const bridge = invoke ?? await waitForInvoke();
        await bridge("complete_tauri_cli", {{ ok: false, payload: normalize(error) }});
      }} catch (_) {{}}
    }}
  }})();
}})();
"#
    ))
}

pub(crate) fn start_tauri_watchdog(app: tauri::AppHandle) -> Result<(), String> {
    let spec = TAURI_RUN
        .lock()
        .map_err(|_| "Tauri CLI state is poisoned".to_string())?
        .clone()
        .ok_or_else(|| "Tauri CLI request is unavailable".to_string())?;
    let bridge_app = app.clone();
    let bridge_spec = spec.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(10)).await;
        if !TAURI_RUN_BRIDGE_READY.load(Ordering::SeqCst)
            && TAURI_RUN_ACTIVE.swap(false, Ordering::SeqCst)
            && !TAURI_RESULT_EMITTED.swap(true, Ordering::SeqCst)
        {
            print_json(&json!({
                "ok": false,
                "command": bridge_spec.id,
                "error": "bridge_unavailable",
                "message": "the hidden Tauri invoke bridge did not initialize"
            }));
            TAURI_RUN_EXIT_CODE.store(9, Ordering::SeqCst);
            terminate_tauri_cli(&bridge_app, 9);
        }
    });
    if let Some(timeout_ms) = spec.timeout_ms {
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(timeout_ms)).await;
            if TAURI_RUN_ACTIVE.swap(false, Ordering::SeqCst)
                && !TAURI_RESULT_EMITTED.swap(true, Ordering::SeqCst)
            {
                print_json(&json!({
                    "ok": false,
                    "command": spec.id,
                    "error": "timeout",
                    "message": format!("read-only command exceeded {timeout_ms} ms; this is a wait limit, not transactional cancellation")
                }));
                TAURI_RUN_EXIT_CODE.store(10, Ordering::SeqCst);
                terminate_tauri_cli(&app, 10);
            }
        });
    }
    Ok(())
}

#[tauri::command]
pub(crate) fn mark_tauri_cli_ready() -> Result<(), String> {
    if !TAURI_RUN_ACTIVE.load(Ordering::SeqCst) {
        return Err("no Tauri CLI command is active".to_string());
    }
    TAURI_RUN_BRIDGE_READY.store(true, Ordering::SeqCst);
    Ok(())
}

#[tauri::command]
pub(crate) fn complete_tauri_cli(
    app: tauri::AppHandle,
    ok: bool,
    payload: Value,
) -> Result<(), String> {
    if !TAURI_RUN_ACTIVE.swap(false, Ordering::SeqCst) {
        return Err("no Tauri CLI command is active".to_string());
    }
    let spec = TAURI_RUN
        .lock()
        .map_err(|_| "Tauri CLI state is poisoned".to_string())?
        .take()
        .ok_or_else(|| "Tauri CLI request is unavailable".to_string())?;
    let code = if ok { 0 } else { 8 };
    // A terminal command acknowledged its dispatch before the runtime started;
    // if its handler does reply, exit with the real code but stay silent so the
    // one-document-per-invocation contract holds.
    if !TAURI_RESULT_EMITTED.swap(true, Ordering::SeqCst) {
        if ok {
            print_json(&json!({ "ok": true, "command": spec.id, "result": payload }));
        } else {
            print_json(&json!({
                "ok": false,
                "command": spec.id,
                "error": "execution_failed",
                "message": payload
            }));
        }
    }
    TAURI_RUN_EXIT_CODE.store(code, Ordering::SeqCst);
    terminate_tauri_cli(&app, code)
}

fn terminate_tauri_cli(app: &tauri::AppHandle, code: i32) -> ! {
    use std::io::Write;
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    app.cleanup_before_exit();
    std::process::exit(code)
}

pub(crate) fn abort_tauri_cli(code: i32, kind: &str, message: &str) -> ! {
    use std::io::Write;
    print_json(&json!({ "ok": false, "error": kind, "message": message }));
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    std::process::exit(code)
}

fn print_json(value: &Value) {
    println!(
        "{}",
        serde_json::to_string(value)
            .unwrap_or_else(|_| "{\"ok\":false,\"error\":\"serialization_failed\"}".to_string())
    );
}

fn fail(code: i32, kind: &str, message: String) -> i32 {
    print_json(&json!({ "ok": false, "error": kind, "message": message }));
    code
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_deadline_does_not_depend_on_tauri_runtime_startup() {
        let settled = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = std::sync::mpsc::channel();

        spawn_backend_deadline(1, settled.clone(), move || {
            sender.send(()).expect("test receiver remains available");
        });

        receiver
            .recv_timeout(std::time::Duration::from_secs(1))
            .expect("ordinary process thread must fire without a Tauri runtime");
        assert!(settled.load(Ordering::SeqCst));
    }

    #[test]
    fn embedded_catalog_is_valid_and_unique() {
        let catalog = catalog().expect("catalog parses");
        assert!(catalog.commands.len() > 400);
        let mut ids: Vec<&str> = catalog
            .commands
            .iter()
            .map(|entry| entry.id.as_str())
            .collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), catalog.commands.len());
    }

    #[test]
    fn destructive_and_read_only_commands_are_separated() {
        let catalog = catalog().expect("catalog parses");
        let get_shellbags = catalog
            .commands
            .iter()
            .find(|entry| entry.id == "backend:Get-ShellBags")
            .unwrap();
        let clear_shellbags = catalog
            .commands
            .iter()
            .find(|entry| entry.id == "backend:Clear-ShellBags")
            .unwrap();
        assert_eq!(classify_risk(get_shellbags), Risk::ReadOnly);
        assert_eq!(classify_risk(clear_shellbags), Risk::Destructive);
        assert!(describe(clear_shellbags)
            .confirmation
            .unwrap()
            .starts_with("DESTROY:"));

        let cleanup_summary = catalog
            .commands
            .iter()
            .find(|entry| entry.id == "backend:Get-CleanupSummaryAllUsers")
            .unwrap();
        assert_eq!(classify_risk(cleanup_summary), Risk::ReadOnly);

        let backup_status = catalog
            .commands
            .iter()
            .find(|entry| entry.id == "backend:Get-EncryptedBackupTargetStatus")
            .unwrap();
        let backup_provision = catalog
            .commands
            .iter()
            .find(|entry| entry.id == "backend:Provision-EncryptedBackupTarget")
            .unwrap();
        let backup_clear = catalog
            .commands
            .iter()
            .find(|entry| entry.id == "backend:Clear-EncryptedBackupTarget")
            .unwrap();
        assert_eq!(classify_risk(backup_status), Risk::ReadOnly);
        assert_eq!(classify_risk(backup_provision), Risk::Mutating);
        assert_eq!(classify_risk(backup_clear), Risk::Destructive);

        let scan_and_clean = GeneratedCommand {
            id: "tauri:scan_and_clean".to_string(),
            name: "scan_and_clean".to_string(),
            transport: Transport::Tauri,
            registered: true,
            debug_only: false,
            frontend_references: vec![],
        };
        assert_eq!(classify_risk(&scan_and_clean), Risk::Destructive);

        for id in [
            "tauri:lockdown",
            "tauri:full_lockdown",
            "tauri:fire_flow",
            "tauri:run_backend_script",
            "tauri:test_pro_dispatch",
        ] {
            let entry = catalog
                .commands
                .iter()
                .find(|entry| entry.id == id)
                .unwrap();
            assert_eq!(classify_risk(entry), Risk::Destructive, "{id}");
            assert_eq!(
                describe(entry).confirmation,
                Some(format!("DESTROY:{id}")),
                "{id}"
            );
        }

        let export_report = GeneratedCommand {
            id: "backend:Export-Report".to_string(),
            name: "Export-Report".to_string(),
            transport: Transport::BackendScript,
            registered: true,
            debug_only: false,
            frontend_references: vec![],
        };
        assert_eq!(classify_risk(&export_report), Risk::Mutating);
    }

    /// The name heuristic cannot be trusted to notice a catastrophic command on
    /// its own: `fleet_connect` and `internet_kill_switch_set` read as ordinary
    /// mutations. `authz::DESTRUCTIVE_COMMANDS` is the list CI already enforces,
    /// so binding the CLI to it means adding a command there is enough to make
    /// the CLI demand `DESTROY:` too.
    #[test]
    fn authoritative_destructive_registry_outranks_the_name_heuristic() {
        let catalog = catalog().expect("catalog parses");
        let mut checked = 0;
        for (name, _) in crate::authz::DESTRUCTIVE_COMMANDS {
            let id = format!("tauri:{name}");
            // Not every registered destructive action is a Tauri handler in Free
            // (`run_destruct_step` is an internal dispatch to the Pro sidecar).
            let Some(entry) = catalog.commands.iter().find(|entry| entry.id == id) else {
                continue;
            };
            assert_eq!(classify_risk(entry), Risk::Destructive, "{id}");
            assert_eq!(
                describe(entry).confirmation,
                Some(format!("DESTROY:{id}")),
                "{id}"
            );
            checked += 1;
        }
        assert!(
            checked >= 6,
            "expected the registry to cover Tauri handlers, matched only {checked}"
        );
    }

    /// Each of these was verified against its handler body, not its name.
    #[test]
    fn handlers_that_misreport_their_effect_use_their_verified_risk() {
        let catalog = catalog().expect("catalog parses");
        for (id, expected) in RISK_OVERRIDES {
            let entry = catalog
                .commands
                .iter()
                .find(|entry| entry.id == *id)
                .unwrap_or_else(|| panic!("{id} left the catalog; re-verify its risk"));
            assert_eq!(classify_risk(entry), *expected, "{id}");
        }
        // search_rename_file calls std::fs::rename, so it must never be granted
        // the no-confirmation read-only tier its `search_` prefix implies.
        let rename = catalog
            .commands
            .iter()
            .find(|entry| entry.id == "tauri:search_rename_file")
            .unwrap();
        assert_eq!(
            describe(rename).confirmation,
            Some("RUN:tauri:search_rename_file".to_string())
        );
    }

    /// The destructive prefix lists are anchored at the start of the name, so
    /// the whole `Invoke-*Erase` family read as ordinary mutations.
    #[test]
    fn irreversible_erasure_is_destructive_wherever_the_word_appears() {
        let catalog = catalog().expect("catalog parses");
        for id in [
            "backend:Invoke-7Erase",
            "backend:Invoke-UnallocatedSpaceErase",
            "backend:Invoke-CrashDumpErase",
            "backend:Invoke-PreviousWindowsInstallErase",
        ] {
            let entry = catalog
                .commands
                .iter()
                .find(|entry| entry.id == id)
                .unwrap_or_else(|| panic!("{id} missing from catalog"));
            assert_eq!(classify_risk(entry), Risk::Destructive, "{id}");
        }
        // ...but the read-only allowlist still wins, so listing erase schedules
        // is not dragged into the destructive tier by the same word.
        let schedules = catalog
            .commands
            .iter()
            .find(|entry| entry.id == "backend:Get-AutoEraseSchedules")
            .unwrap();
        assert_eq!(classify_risk(schedules), Risk::ReadOnly);
    }

    /// The generator derives this from `#[cfg(debug_assertions)]`; a hardcoded
    /// copy in this file used to be able to drift silently past every gate.
    #[test]
    fn debug_only_flags_come_from_the_generated_catalog() {
        let catalog = catalog().expect("catalog parses");
        let flagged: Vec<&str> = catalog
            .commands
            .iter()
            .filter(|entry| entry.debug_only)
            .map(|entry| entry.id.as_str())
            .collect();
        assert_eq!(
            flagged,
            [
                "tauri:dev_reset_state",
                "tauri:dev_simulate_event",
                "tauri:open_devtools",
                "tauri:test_pro_dispatch"
            ]
        );
        assert!(catalog
            .commands
            .iter()
            .filter(|entry| entry.debug_only)
            .all(|entry| entry.transport == Transport::Tauri));
    }

    #[test]
    fn params_accept_primitives_and_structures_without_shell_parsing() {
        let params = parse_params(r#"{"flag":true,"count":2,"names":["a","b"]}"#).unwrap();
        assert_eq!(params["flag"], true);
        assert_eq!(params["count"], 2);
        assert_eq!(params["names"], json!(["a", "b"]));

        let backend = backend_params(&params).unwrap();
        assert_eq!(backend["flag"], "true");
        assert_eq!(backend["count"], "2");
        assert_eq!(backend["names"], r#"["a","b"]"#);
    }

    #[test]
    fn every_registered_tauri_handler_is_cli_executable() {
        let catalog = catalog().expect("catalog parses");
        let tauri_commands: Vec<_> = catalog
            .commands
            .iter()
            .filter(|entry| entry.transport == Transport::Tauri)
            .collect();
        // Keep this snapshot count intentional: the generated catalog is the
        // authority, but a count change must be reviewed with the handler
        // registrations rather than silently widening the CLI surface.
        assert_eq!(tauri_commands.len(), 442);
        assert!(tauri_commands.iter().all(|entry| entry.registered));
        for name in [
            "decoy_read_audit_status",
            "fleet_report_local_alert",
            "fleet_report_privacy_shield_status",
            "fleet_sync_shield_state",
            "list_search_known_folders",
            "list_search_storage_roots",
            "open_event_log",
            "set_decoy_read_audit_enabled",
        ] {
            assert!(
                tauri_commands.iter().any(|entry| entry.name == name),
                "generated CLI catalog is missing registered handler {name}"
            );
        }
        assert_eq!(
            tauri_commands
                .iter()
                .filter(|entry| entry.debug_only)
                .count(),
            4
        );
        assert!(tauri_commands
            .iter()
            .filter(|entry| available_in_this_build(entry))
            .all(|entry| match classify_risk(entry) {
                Risk::ReadOnly => classify_headless_support(entry) == HeadlessSupport::Executable,
                Risk::Mutating | Risk::Destructive => {
                    classify_headless_support(entry) == HeadlessSupport::UiOnly
                }
            }));
        assert_eq!(
            tauri_commands
                .iter()
                .filter(|entry| available_in_this_build(entry))
                .count(),
            if cfg!(debug_assertions) { 442 } else { 438 }
        );
    }

    /// `--confirm --dry-run` used to absorb `--dry-run` as the confirmation
    /// value. On a read-only command no confirmation is checked, so the operator
    /// asked for a preview and got a live execution. The same swallow could
    /// smuggle `--safe-copy` through to the argv scan in `lib.rs::run`.
    #[test]
    fn a_flag_is_never_accepted_as_an_option_value() {
        let argv: Vec<String> = ["run", "tauri:get_settings", "--confirm", "--dry-run"]
            .iter()
            .map(|arg| (*arg).to_string())
            .collect();
        let error = match parse_action(&argv) {
            Err(error) => error,
            Ok(_) => panic!("`--confirm --dry-run` must not parse"),
        };
        assert!(error.contains("--confirm"), "{error}");
        assert!(error.contains("--dry-run"), "{error}");

        for flag in ["--params", "--timeout-ms", "--confirm"] {
            let argv: Vec<String> = ["run", "tauri:get_settings", flag, "--safe-copy"]
                .iter()
                .map(|arg| (*arg).to_string())
                .collect();
            assert!(parse_action(&argv).is_err(), "{flag} accepted a flag value");
        }

        let argv: Vec<String> = ["run", "tauri:get_settings", "--dry-run"]
            .iter()
            .map(|arg| (*arg).to_string())
            .collect();
        assert!(matches!(
            parse_action(&argv),
            Ok(Action::Run(RunRequest { dry_run: true, .. }))
        ));
    }

    /// Params reach the hidden webview as a parsed JSON document, so a key named
    /// `__proto__` stays an ordinary parameter instead of setting the prototype.
    #[test]
    fn params_reach_the_webview_as_parsed_json_not_object_syntax() {
        *TAURI_RUN.lock().unwrap() = Some(TauriRunSpec {
            id: "tauri:get_settings".to_string(),
            command: "get_settings".to_string(),
            params: json!({ "__proto__": { "polluted": true }, "quote": "a\"b" }),
            timeout_ms: None,
        });
        let script = tauri_initialization_script().expect("script builds");
        *TAURI_RUN.lock().unwrap() = None;
        assert!(script.contains("JSON.parse("), "{script}");
        assert!(
            !script.contains("const params = {\""),
            "params must not be spliced as object syntax"
        );
        // The JSON document is embedded as a JS string literal, so its own
        // quotes arrive escaped rather than terminating the literal early.
        assert!(script.contains("__proto__"));
        assert!(script.contains("\\\"polluted\\\""), "{script}");
    }

    #[test]
    fn only_explicit_cli_verbs_replace_the_gui_launch() {
        assert!(is_cli_invocation(&["commands".to_string()]));
        assert!(is_cli_invocation(&["audit".to_string()]));
        assert!(!is_cli_invocation(&[]));
        assert!(!is_cli_invocation(&["--scrub".to_string()]));
        assert!(!is_cli_invocation(&["--safe-paste".to_string()]));
    }
}
