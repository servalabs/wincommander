// ══════════════════════════════════════════════════════════════════════════
// Server Apps — Native Multiwebview Management
// ══════════════════════════════════════════════════════════════════════════
// ARCHITECTURE: Instead of iframes (which are blocked by X-Frame-Options /
// CSP frame-ancestors on most self-hosted apps), we create native WebView2
// child webviews inside the main window using Tauri v2's unstable multiwebview API.
//
// Each server app (Immich, Nextcloud, Syncthing, etc.) gets its own WebView2
// instance with a label like "server-app-gallery". This bypasses all iframe
// restrictions since each webview is a top-level browsing context.
//
// CSS injection for whitelabeling is done via `initialization_script()` which
// runs a DOMContentLoaded listener that appends a <style> tag with the user's
// custom CSS. This runs before the page's own scripts on every navigation.
//
// LEARNING: Child webviews render ON TOP of the main webview content (z-order).
// The frontend must position the webview bounds precisely to avoid overlapping
// the sidebar, titlebar, or right sidebar. Use ResizeObserver + invoke to keep
// bounds in sync.
//
// LEARNING: Each WebView2 instance uses ~100-200MB RAM. Create on-demand and
// hide inactive ones rather than pre-creating all. Consider destroying webviews
// that haven't been used in a while.
// ══════════════════════════════════════════════════════════════════════════

use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};
use tauri::{Manager, WebviewBuilder, WebviewUrl};

#[derive(Clone, Copy, PartialEq)]
struct WebviewBounds {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

fn bounds_cache() -> &'static Mutex<HashMap<String, WebviewBounds>> {
    static CACHE: OnceLock<Mutex<HashMap<String, WebviewBounds>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn normalized_bounds(x: f64, y: f64, w: f64, h: f64) -> WebviewBounds {
    WebviewBounds {
        x: x.round() as i32,
        y: y.round() as i32,
        w: w.round() as i32,
        h: h.round() as i32,
    }
}

fn remember_bounds(label: &str, bounds: WebviewBounds) {
    if let Ok(mut cache) = bounds_cache().lock() {
        cache.insert(label.to_string(), bounds);
    }
}

fn bounds_changed(label: &str, bounds: WebviewBounds) -> bool {
    let Ok(mut cache) = bounds_cache().lock() else {
        return true;
    };
    if cache.get(label) == Some(&bounds) {
        return false;
    }
    cache.insert(label.to_string(), bounds);
    true
}

/// Build the initialization_script JS that:
/// 1. Disables browser autofill on all forms/inputs (security — prevents saved
///    credentials from appearing in login forms, which exposes usernames/IDs).
/// 2. Optionally injects custom CSS for whitelabeling.
/// 3. Optionally injects custom JS for branding (logo→text replacement etc.).
fn build_init_script(custom_css: &Option<String>, custom_js: &Option<String>) -> String {
    // Always block autofill — runs on every navigation
    let mut script = r#"
    (function() {
        function _wcNoAutofill(root) {
            root.querySelectorAll('input, select, textarea').forEach(function(el) {
                el.setAttribute('autocomplete', 'off');
                el.setAttribute('data-lpignore', 'true');
                el.setAttribute('data-form-type', 'other');
            });
            root.querySelectorAll('form').forEach(function(f) {
                f.setAttribute('autocomplete', 'off');
            });
        }
        document.addEventListener('DOMContentLoaded', function() {
            _wcNoAutofill(document);
            new MutationObserver(function(muts) {
                muts.forEach(function(m) {
                    m.addedNodes.forEach(function(n) {
                        if (n.querySelectorAll) _wcNoAutofill(n);
                    });
                });
            }).observe(document.body, { childList: true, subtree: true });
        });
    })();
    "#
    .to_string();

    // Append custom CSS injection if provided
    if let Some(css) = custom_css {
        if !css.trim().is_empty() {
            // Emit the CSS as a JSON string literal rather than hand-escaping it
            // into a JS template literal. The old code only escaped `\` and a
            // backtick, so a `${...}` sequence in the CSS was still interpreted
            // as a template-literal substitution and executed as arbitrary JS.
            // serde_json escapes backslashes, quotes, control chars, etc. and the
            // result is a plain double-quoted string with no interpolation.
            let css_literal =
                serde_json::to_string(css.as_str()).unwrap_or_else(|_| "\"\"".to_string());
            script.push_str(&format!(
                r#"
                (function() {{
                    document.addEventListener('DOMContentLoaded', function() {{
                        var style = document.createElement('style');
                        style.setAttribute('data-wincommander', 'custom-css');
                        style.textContent = {};
                        document.head.appendChild(style);
                    }});
                }})();
                "#,
                css_literal
            ));
        }
    }

    // Append custom JS injection if provided.
    // Runs on DOMContentLoaded + MutationObserver for SPA support (Immich/Svelte, etc.).
    // The JS must be idempotent — it may fire many times as the DOM mutates.
    if let Some(js) = custom_js {
        if !js.trim().is_empty() {
            script.push_str("\n(function() {\n");
            script.push_str("  function _wcBrandingTask() {\n");
            script.push_str(js);
            script.push_str("\n  }\n");
            script.push_str("  var _wcTimer;\n");
            script.push_str("  function _wcDebounced() { clearTimeout(_wcTimer); _wcTimer = setTimeout(_wcBrandingTask, 80); }\n");
            script.push_str("  if (document.readyState === 'loading') {\n");
            script.push_str("    document.addEventListener('DOMContentLoaded', function() {\n");
            script.push_str("      _wcBrandingTask();\n");
            script.push_str("      new MutationObserver(_wcDebounced).observe(document.body, {childList:true, subtree:true});\n");
            script.push_str("    });\n");
            script.push_str("  } else {\n");
            script.push_str("    _wcBrandingTask();\n");
            script.push_str("    if (document.body) new MutationObserver(_wcDebounced).observe(document.body, {childList:true, subtree:true});\n");
            script.push_str("  }\n");
            script.push_str("})();\n");
        }
    }

    script
}

/// Open (or show) a webview tab in a native WebView2 child webview.
/// `group` scopes the webview labels — e.g. "server-app" or "productivity".
/// Hides all other {group}-* webviews first so only one is visible at a time.
/// If the webview already exists, it is shown and repositioned (no reload).
/// If it doesn't exist, a new one is created with the given URL and optional CSS.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn open_server_app(
    app: tauri::AppHandle,
    group: String,
    id: String,
    url: String,
    custom_css: Option<String>,
    custom_js: Option<String>,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    // ephemeral: route this webview's storage to a separate per-group
    // data directory and DESTROY whatever was there first. Used by the
    // mesh-login flow so a stale Tailscale session cookie doesn't make
    // the next "Sign In" silently auto-complete without showing the
    // login form. Other groups (server-app, productivity, etc.) keep
    // their persistent cookies for usability.
    ephemeral: Option<bool>,
) -> Result<(), String> {
    let prefix = format!("{}-", group);
    let label = format!("{}{}", prefix, id);
    let is_ephemeral = ephemeral.unwrap_or(false);
    let bounds = normalized_bounds(x, y, w, h);

    // Hide all other webviews in the same group first
    // LEARNING: Child webviews created with window.add_child() are Webview instances,
    // NOT WebviewWindow instances. Must use app.webviews() to find them.
    // app.webviews() returns Vec<(String, Webview)> tuples.
    for (lbl, wv) in app.webviews() {
        if lbl.starts_with(&prefix) && lbl != label {
            let _ = wv.hide();
        }
    }

    // Ephemeral path: if a webview with this label already exists from
    // a previous Sign In, CLOSE it so the new one starts with the erased
    // data directory. Without this we'd reuse the existing webview and
    // its in-memory session state would defeat the cookie erase below.
    if is_ephemeral {
        if let Some(wv) = app.get_webview(&label) {
            let _ = wv.close();
        }
    } else if let Some(wv) = app.get_webview(&label) {
        // Persistent path: existing webview wins — just show + reposition.
        if bounds_changed(&label, bounds) {
            wv.set_position(tauri::LogicalPosition::new(x, y))
                .map_err(|e| e.to_string())?;
            wv.set_size(tauri::LogicalSize::new(w, h))
                .map_err(|e| e.to_string())?;
        }
        wv.show().map_err(|e| e.to_string())?;
        let _ = wv.set_focus();
        return Ok(());
    }

    // Create a new webview inside the main window
    let window = app
        .get_window("main")
        .ok_or_else(|| "Main window not found".to_string())?;

    // Validate URL format before passing to WebviewUrl::External
    // tauri::WebviewUrl::External expects a url::Url; tauri re-exports it as tauri::Url
    let parsed_url: tauri::Url = url
        .parse()
        .map_err(|e| format!("Invalid URL '{}': {}", url, e))?;

    let init_script = build_init_script(&custom_css, &custom_js);

    // Use per-user data directory for WebView2 instances to avoid cross-user lock conflicts.
    // For ephemeral groups (mesh-login), use a SEPARATE per-group subdir
    // and erase it first so cookies never persist across Sign In attempts.
    // Otherwise share the long-lived ServerApps directory so persistent
    // logins (Immich, Nextcloud, etc.) survive WinCommander restarts.
    let webview_data_dir = {
        let base = crate::paths::user_data_dir()
            .unwrap_or_else(|_| std::env::temp_dir().join("WinCommander"))
            .join("WebView2");
        if is_ephemeral {
            let p = base.join("Ephemeral").join(&group);
            // Best-effort erase — ignore failures (e.g. file in use from
            // a still-shutting-down webview). WebView2 will recreate
            // whatever subset it needs on first navigation.
            let _ = std::fs::remove_dir_all(&p);
            p
        } else {
            base.join("ServerApps")
        }
    };

    let builder =
        WebviewBuilder::new(&label, WebviewUrl::External(parsed_url))
            .transparent(false)
            // Force desktop user-agent so self-hosted apps (Immich, HA, Nextcloud)
            // don't serve mobile layouts. Without this, some apps detect the WebView2
            // default UA as a mobile/embedded browser and render phone-sized UI.
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
            .initialization_script(&init_script)
            .data_directory(webview_data_dir);

    window
        .add_child(
            builder,
            tauri::LogicalPosition::new(x, y),
            tauri::LogicalSize::new(w, h),
        )
        .map_err(|e| format!("Failed to create webview '{}': {}", label, e))?;
    remember_bounds(&label, bounds);

    Ok(())
}

/// Hide all {group}-* webviews.
/// Called when the user navigates away from a panel.
#[tauri::command]
pub async fn hide_all_server_apps(app: tauri::AppHandle, group: String) -> Result<(), String> {
    let prefix = format!("{}-", group);
    for (lbl, wv) in app.webviews() {
        if lbl.starts_with(&prefix) {
            let _ = wv.hide();
        }
    }
    Ok(())
}

/// Resize / reposition a webview tab.
/// Called on window resize via ResizeObserver in the frontend.
#[tauri::command]
pub async fn resize_server_app(
    app: tauri::AppHandle,
    group: String,
    id: String,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) -> Result<(), String> {
    let label = format!("{}-{}", group, id);
    if let Some(wv) = app.get_webview(&label) {
        let bounds = normalized_bounds(x, y, w, h);
        if !bounds_changed(&label, bounds) {
            return Ok(());
        }
        wv.set_position(tauri::LogicalPosition::new(x, y))
            .map_err(|e| e.to_string())?;
        wv.set_size(tauri::LogicalSize::new(w, h))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Close and destroy a specific webview tab to free memory.
#[tauri::command]
pub async fn close_server_app(
    app: tauri::AppHandle,
    group: String,
    id: String,
) -> Result<(), String> {
    let label = format!("{}-{}", group, id);
    if let Some(wv) = app.get_webview(&label) {
        wv.close().map_err(|e| e.to_string())?;
    }
    if let Ok(mut cache) = bounds_cache().lock() {
        cache.remove(&label);
    }
    Ok(())
}

/// Close all {group}-* webviews. Used on app exit or panel hard-reset.
#[tauri::command]
pub async fn close_all_server_apps(app: tauri::AppHandle, group: String) -> Result<(), String> {
    let prefix = format!("{}-", group);
    for (lbl, wv) in app.webviews() {
        if lbl.starts_with(&prefix) {
            let _ = wv.close();
            if let Ok(mut cache) = bounds_cache().lock() {
                cache.remove(&lbl);
            }
        }
    }
    Ok(())
}
