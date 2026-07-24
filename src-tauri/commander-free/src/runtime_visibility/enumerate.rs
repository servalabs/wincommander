// Services + scheduled-tasks enumeration via the standard Windows CLIs
// (sc.exe, schtasks.exe). Read-only; no admin. We deliberately don't use a
// crate for windows-service here because the data we need (name, display
// name, state, start type) all comes back from `sc query` cheaply enough,
// and shelling out keeps the surface area testable without an extra dep.

use serde::Serialize;
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use std::process::Command;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ServiceInfo {
    pub name: String,
    pub display_name: String,
    pub state: String,      // "RUNNING" | "STOPPED" | …
    pub start_type: String, // "AUTO_START" | "DEMAND_START" | "DISABLED" | …
    pub binary_path: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledTaskInfo {
    pub task_name: String, // full path, e.g. "\Microsoft\Windows\…\Task"
    pub status: String,
    pub next_run: Option<String>,
    pub last_run: Option<String>,
    pub author: Option<String>,
    pub action: Option<String>,
}

#[cfg(target_os = "windows")]
fn run_console_cmd(program: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new(program)
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("{} spawn failed: {}", program, e))?;
    if !out.status.success() {
        // sc.exe writes useful info to stdout even on non-zero exit codes
        // (e.g. when the query ends), so we accept the bytes regardless and
        // only error when stdout is empty.
        if out.stdout.is_empty() {
            return Err(format!(
                "{} exit {}: {}",
                program,
                out.status.code().unwrap_or(-1),
                String::from_utf8_lossy(&out.stderr)
            ));
        }
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

#[cfg(not(target_os = "windows"))]
fn run_console_cmd(_program: &str, _args: &[&str]) -> Result<String, String> {
    Err("Windows-only".into())
}

// ─── Services ────────────────────────────────────────────────────────────

/// `sc query type= service state= all` prints stanzas like:
///   SERVICE_NAME: Foo
///   DISPLAY_NAME: Foo Service
///           TYPE               : 10  WIN32_OWN_PROCESS
///           STATE              : 4  RUNNING
///           …
/// We collect SERVICE_NAME, DISPLAY_NAME, STATE; then issue `sc qc <name>`
/// for the start-type + binary-path of services that look "user-installed"
/// (skip the Windows-bundled ones to keep the list short).
fn parse_sc_query(text: &str) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    let mut name = String::new();
    let mut display = String::new();
    let mut state = String::new();

    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("SERVICE_NAME:") {
            if !name.is_empty() {
                out.push((
                    std::mem::take(&mut name),
                    std::mem::take(&mut display),
                    std::mem::take(&mut state),
                ));
            }
            name = rest.trim().to_string();
        } else if let Some(rest) = trimmed.strip_prefix("DISPLAY_NAME:") {
            display = rest.trim().to_string();
        } else if let Some(rest) = trimmed.strip_prefix("STATE") {
            // "STATE              : 4  RUNNING" — strip the alignment spaces
            // BEFORE the colon, then the colon itself, then take the second
            // whitespace-separated token ("RUNNING" after "4").
            let rest = rest.trim().strip_prefix(':').unwrap_or(rest).trim();
            if let Some(word) = rest.split_whitespace().nth(1) {
                state = word.to_string();
            }
        }
    }
    if !name.is_empty() {
        out.push((name, display, state));
    }
    out
}

/// Best-effort heuristic for "this is a user-installed service worth showing
/// in the runtime list" vs a stock Windows service. We could parse the
/// binary path and check against `%SystemRoot%\System32\` but that needs
/// a `sc qc` per service which slows the panel a lot. Instead: drop the
/// well-known prefixes that Windows itself uses for its bundled services.
fn looks_like_user_service(name: &str, display: &str) -> bool {
    let n = name.to_lowercase();
    let d = display.to_lowercase();
    const BLOCKLIST: &[&str] = &[
        "windows ",
        "microsoft ",
        "ms ",
        "azure ",
        "xbox ",
        "windefend",
        "wuauserv",
        "bits",
        "wsearch",
        "dhcp",
        "dnscache",
        "rpcss",
        "rpceptmapper",
        "themes",
        "winrm",
        "spooler",
        "audiosrv",
        "audioendpointbuilder",
        "lanmanserver",
        "lanmanworkstation",
        "netman",
        "netprofm",
        "nlasvc",
        "schedule",
        "ssdpsrv",
        "tabletinputservice",
        "tapisrv",
        "termservice",
        "trkwks",
        "ui0detect",
        "uxsms",
        "vaultsvc",
        "w32time",
        "wbiosrvc",
        "wcmsvc",
        "wcncsvc",
        "wecsvc",
        "wercplsupport",
        "windowsazureguestagent",
        "winhttpautoproxysvc",
        "wlansvc",
        "wlidsvc",
        "wmpnetworksvc",
        "workstation",
        "wuauserv",
        "wzcsvc",
        "applockerfltr",
        "appxsvc",
        "appidsvc",
        "appmgmt",
        "appreadiness",
        "audiosrv",
        "axinstsvc",
        "background",
        "biosrvc",
        "biometric",
        "bluetooth",
        "bts",
        "cdpsvc",
        "cdpusersvc",
        "certpropsvc",
        "clipsvc",
        "cryptsvc",
        "dcomlaunch",
        "defragsvc",
        "dhcp",
        "diagtrack",
        "dispbrokerdesktopsvc",
        "dmwappushservice",
        "dssvc",
        "dusmsvc",
        "eaphost",
        "edgeupdate",
        "embeddedmode",
        "enterpriseappmgmtsvc",
        "eventlog",
        "eventsystem",
        "fdphost",
        "fdrespub",
        "fontcache",
        "gpsvc",
        "graphicsperfsvc",
        "hns",
        "hvhost",
        "iaasagent",
        "iass",
        "icssvc",
        "idaservice",
        "ikeext",
        "instsvc",
        "intelaudioservice",
        "iphlpsvc",
        "irmon",
        "keyiso",
        "klsetuphandler",
        "klserveravsvc",
    ];
    if BLOCKLIST
        .iter()
        .any(|p| n.starts_with(p) || d.starts_with(p))
    {
        return false;
    }
    true
}

fn parse_sc_qc(text: &str) -> (String, Option<String>) {
    let mut start = String::new();
    let mut bin: Option<String> = None;
    for line in text.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("START_TYPE") {
            let rest = rest.trim().strip_prefix(':').unwrap_or(rest).trim();
            if let Some(word) = rest.split_whitespace().nth(1) {
                start = word.to_string();
            }
        } else if let Some(rest) = t.strip_prefix("BINARY_PATH_NAME") {
            let rest = rest.trim().strip_prefix(':').unwrap_or(rest).trim();
            if !rest.is_empty() {
                bin = Some(rest.to_string());
            }
        }
    }
    (start, bin)
}

#[tauri::command]
pub fn enumerate_services() -> Result<Vec<ServiceInfo>, String> {
    let all = run_console_cmd("sc.exe", &["query", "type=", "service", "state=", "all"])?;
    let triples = parse_sc_query(&all);

    let filtered: Vec<_> = triples
        .into_iter()
        .filter(|(n, d, _)| looks_like_user_service(n, d))
        .collect();

    let mut out = Vec::with_capacity(filtered.len());
    for (name, display, state) in filtered.into_iter().take(120) {
        let qc = run_console_cmd("sc.exe", &["qc", &name]).unwrap_or_default();
        let (start_type, binary_path) = parse_sc_qc(&qc);
        out.push(ServiceInfo {
            name,
            display_name: display,
            state,
            start_type,
            binary_path,
        });
    }

    out.sort_by_key(|a| a.display_name.to_lowercase());
    Ok(out)
}

// ─── Scheduled tasks ─────────────────────────────────────────────────────

/// `schtasks /query /fo CSV /v` produces a CSV that, even with /v, has only
/// the columns we need plus a lot of noise. We just take the basics.
fn parse_schtasks_csv(text: &str) -> Vec<ScheduledTaskInfo> {
    let mut out = Vec::new();
    let mut header_indices: Option<(usize, usize, usize, usize, usize, Option<usize>)> = None;
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let cells = split_csv_row(line);

        if header_indices.is_none() {
            let mut idx = (0usize, 0usize, 0usize, 0usize, 0usize, None);
            let mut got = [false; 5];
            for (i, c) in cells.iter().enumerate() {
                match c.as_str() {
                    "TaskName" => {
                        idx.0 = i;
                        got[0] = true;
                    }
                    "Status" => {
                        idx.1 = i;
                        got[1] = true;
                    }
                    "Next Run Time" => {
                        idx.2 = i;
                        got[2] = true;
                    }
                    "Last Run Time" => {
                        idx.3 = i;
                        got[3] = true;
                    }
                    "Author" => {
                        idx.4 = i;
                        got[4] = true;
                    }
                    "Task To Run" => {
                        idx.5 = Some(i);
                    }
                    _ => {}
                }
            }
            if got[0] && got[1] {
                header_indices = Some(idx);
            }
            continue;
        }

        let (ti, si, nri, lri, ai, action_i) = header_indices.unwrap();
        if cells.len() <= ti {
            continue;
        }
        let task_name = cells[ti].clone();
        // Skip header repeats and folder summary rows.
        if task_name == "TaskName" || task_name.is_empty() {
            continue;
        }
        out.push(ScheduledTaskInfo {
            task_name,
            status: cells.get(si).cloned().unwrap_or_default(),
            next_run: cells.get(nri).cloned().filter(|s| !s.is_empty()),
            last_run: cells.get(lri).cloned().filter(|s| !s.is_empty()),
            author: cells.get(ai).cloned().filter(|s| !s.is_empty()),
            action: action_i.and_then(|i| cells.get(i).cloned().filter(|s| !s.is_empty())),
        });
    }
    out
}

/// Minimal CSV splitter — handles quoted cells with embedded commas, which
/// is all `schtasks /fo CSV` produces. No embedded quotes inside quoted
/// fields (schtasks doesn't emit them).
fn split_csv_row(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    for c in line.chars() {
        match c {
            '"' => in_quotes = !in_quotes,
            ',' if !in_quotes => {
                out.push(std::mem::take(&mut cur));
            }
            _ => cur.push(c),
        }
    }
    out.push(cur);
    out
}

fn looks_like_user_task(task: &ScheduledTaskInfo) -> bool {
    let p = task.task_name.to_lowercase();
    // Windows ships hundreds of tasks under \Microsoft\Windows\. Most users
    // don't manage these. Surface the rest.
    if p.starts_with("\\microsoft\\") {
        return false;
    }
    !p.is_empty()
}

#[tauri::command]
pub fn enumerate_scheduled_tasks() -> Result<Vec<ScheduledTaskInfo>, String> {
    let text = run_console_cmd("schtasks.exe", &["/query", "/fo", "CSV", "/v"])?;
    let all = parse_schtasks_csv(&text);
    Ok(all.into_iter().filter(looks_like_user_task).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sc_query_three_services() {
        let sample = r#"
SERVICE_NAME: Foo
DISPLAY_NAME: Foo Display
        TYPE               : 10  WIN32_OWN_PROCESS
        STATE              : 4  RUNNING
        WIN32_EXIT_CODE    : 0

SERVICE_NAME: Bar
DISPLAY_NAME: Bar
        STATE              : 1  STOPPED
"#;
        let parsed = parse_sc_query(sample);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].0, "Foo");
        assert_eq!(parsed[0].2, "RUNNING");
        assert_eq!(parsed[1].0, "Bar");
        assert_eq!(parsed[1].2, "STOPPED");
    }

    #[test]
    fn parses_sc_qc_start_and_path() {
        let sample = r#"
[SC] QueryServiceConfig SUCCESS

SERVICE_NAME: Foo
        TYPE               : 10  WIN32_OWN_PROCESS
        START_TYPE         : 2   AUTO_START
        BINARY_PATH_NAME   : C:\Program Files\Foo\foo.exe -s
"#;
        let (s, b) = parse_sc_qc(sample);
        assert_eq!(s, "AUTO_START");
        assert_eq!(b.as_deref(), Some("C:\\Program Files\\Foo\\foo.exe -s"));
    }

    #[test]
    fn csv_split_handles_quoted_commas() {
        let cells = split_csv_row(r#""Foo, Bar","baz","",qux"#);
        assert_eq!(cells, vec!["Foo, Bar", "baz", "", "qux"]);
    }

    #[test]
    fn user_task_filter_drops_microsoft_tasks() {
        let mut t = ScheduledTaskInfo {
            task_name: "\\Microsoft\\Windows\\Foo\\Bar".into(),
            status: "Ready".into(),
            next_run: None,
            last_run: None,
            author: None,
            action: None,
        };
        assert!(!looks_like_user_task(&t));
        t.task_name = "\\MyApp\\Updater".into();
        assert!(looks_like_user_task(&t));
    }
}
