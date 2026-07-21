#[cfg(windows)]
use std::path::{Path, PathBuf};
use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, MutexGuard,
    },
    thread,
    time::Duration,
};
use tauri::{
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu},
    AppHandle, Listener, Manager, WebviewUrl, WindowEvent, Wry,
};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tauri_plugin_opener::OpenerExt;
use tauri_plugin_updater::UpdaterExt;
#[cfg(windows)]
use winreg::{
    enums::{HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE, REG_BINARY},
    RegKey, RegValue,
};

#[cfg(windows)]
mod window_tracking;

const MENU_LOGIN: &str = "login";
const MENU_LOGOUT: &str = "logout";
const MENU_LOGGED_IN_AS: &str = "logged-in-as";
const MENU_TIMELINE_SETTINGS: &str = "timeline-settings";
const MENU_ALWAYS_ON_TOP: &str = "always-on-top";
const MENU_HIDE_WHEN_UNFOCUSED: &str = "hide-when-unfocused";
#[cfg(windows)]
const MENU_EXCLUDE_FROM_CAPTURE: &str = "exclude-from-capture";
const MENU_ATTACH_WINDOW: &str = "attach-window";
#[cfg(windows)]
const MENU_ATTACHED_PROCESS: &str = "attached-process";
#[cfg(windows)]
const MENU_START_WITH_WINDOWS: &str = "start-with-windows";
const MENU_OPEN_SITE: &str = "open-naphwiki";
const MENU_BECOME_PREMIUM: &str = "become-premium";
const MENU_CLOSE: &str = "close";
const CONTEXT_MENU_EVENT: &str = "timeline-context-menu";
const ORIENTATION_EVENT: &str = "timeline-orientation-change";
const SETTINGS_CHANGE_EVENT: &str = "timeline-settings-change";
const CHANGELOG_STATE_FILE: &str = "last-changelog-version.txt";
const VERSION_HISTORY_WINDOW: &str = "version-history";
const VERSION_HISTORY_HTML: &str = r##"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Naphwiki Timeline version history</title>
  <style>
    :root { color-scheme: dark; font-family: "Segoe UI", sans-serif; }
    * { box-sizing: border-box; }
    html, body { min-height: 100%; margin: 0; background: #100d0a; color: #ded6ca; }
    body { overflow-y: auto; }
    header {
      position: sticky; top: 0; z-index: 2; padding: 22px 24px 18px;
      background: linear-gradient(180deg, rgba(34, 27, 20, 0.98), rgba(20, 16, 12, 0.96));
      border-bottom: 1px solid #5a4026; box-shadow: 0 8px 24px rgba(0, 0, 0, 0.28);
    }
    h1 { margin: 0; color: #f0a847; font-size: 1.4rem; }
    header p { margin: 7px 0 0; color: #a99f92; font-size: 0.86rem; }
    main { display: grid; gap: 14px; padding: 18px 24px 28px; }
    article {
      padding: 16px 18px; background: linear-gradient(180deg, #211a14, #17120e);
      border: 1px solid #46321f; border-radius: 8px; box-shadow: 0 8px 22px rgba(0, 0, 0, 0.2);
    }
    article.current { border-color: #b66c27; box-shadow: 0 0 0 1px rgba(240, 138, 44, 0.18); }
    h2 { display: flex; align-items: center; gap: 9px; margin: 0 0 10px; color: #f0bd72; font-size: 1.02rem; }
    .badge {
      padding: 3px 7px; border-radius: 999px; background: #7b351d; color: #ffd7aa;
      font-size: 0.65rem; font-weight: 700; letter-spacing: 0.04em; text-transform: uppercase;
    }
    ul { display: grid; gap: 7px; margin: 0; padding-left: 19px; }
    li { color: #c8c0b5; font-size: 0.86rem; line-height: 1.42; }
    ::-webkit-scrollbar { width: 10px; }
    ::-webkit-scrollbar-track { background: #100d0a; }
    ::-webkit-scrollbar-thumb { background: #694322; border: 2px solid #100d0a; border-radius: 999px; }
  </style>
</head>
<body>
  <header>
    <h1>Version history</h1>
    <p>Updates to Naphwiki Timeline, newest first.</p>
  </header>
  <main>
    <article class="current">
      <h2>Version 0.2.2 <span class="badge">Current</span></h2>
      <ul>
        <li>Allowed vertical timelines to shrink to a compact 20 px width.</li>
        <li>Added premium supporter and Close actions to the context menu.</li>
        <li>Improved compact vertical event labels and gradient direction.</li>
      </ul>
    </article>
    <article>
      <h2>Version 0.2.1</h2>
      <ul>
        <li>Restored the normal Windows shadow in transparent-surroundings mode.</li>
        <li>Replaced the single-version notification with this scrollable version history.</li>
      </ul>
    </article>
    <article>
      <h2>Version 0.2.0</h2>
      <ul>
        <li>Added premium appearance controls for bar opacity, vertical layout, default event colors, and edge-aligned current time.</li>
        <li>Added transparent surroundings and a switch for the Hot Purge animation.</li>
        <li>Made vertical events move upward, allowed 20 px compact layouts, and added live settings previews.</li>
      </ul>
    </article>
    <article>
      <h2>Version 0.1.6</h2>
      <ul><li>Added automatic window resizing when switching between horizontal and vertical timelines.</li></ul>
    </article>
    <article>
      <h2>Version 0.1.5</h2>
      <ul><li>Added the in-app timeline settings window and refreshed the timeline after settings or login changes.</li></ul>
    </article>
    <article>
      <h2>Version 0.1.4</h2>
      <ul><li>Added optional capture exclusion on supported Windows versions and reorganized the context menu.</li></ul>
    </article>
    <article>
      <h2>Version 0.1.3</h2>
      <ul><li>Changed the default target to L2.bin, added Windows startup support, and remembered the attached window position.</li></ul>
    </article>
    <article>
      <h2>Version 0.1.2</h2>
      <ul><li>Added window attachment, focus-aware topmost behavior, and automatic updates from GitHub releases.</li></ul>
    </article>
    <article>
      <h2>Version 0.1.1</h2>
      <ul><li>Added drag-anywhere movement, invisible resize handles, and improved account controls.</li></ul>
    </article>
    <article>
      <h2>Version 0.1.0</h2>
      <ul><li>Initial release of the Naphwiki Timeline Windows overlay.</li></ul>
    </article>
  </main>
</body>
</html>"##;

const SITE_URL: &str = "https://www.naphwiki.com";
const PATREON_MEMBERSHIP_URL: &str = "https://www.patreon.com/cw/Naphwiki/membership";
const LOGIN_URL: &str = "https://www.naphwiki.com/auth/discord?returnTo=%2Ftimeline";
const SETTINGS_URL: &str = "https://www.naphwiki.com/timeline/settings";
#[cfg(windows)]
const DEFAULT_TARGET_PROCESS: &str = "L2.bin";
#[cfg(windows)]
const AUTOSTART_ARGUMENT: &str = "--autostart";
#[cfg(windows)]
const TRACKING_SETTINGS_FILE: &str = "window-tracking.json";
#[cfg(windows)]
const MAX_TRACKING_SETTINGS_BYTES: u64 = 4 * 1024;
#[cfg(windows)]
const WINDOWS_RUN_KEY: &str = "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Run";
#[cfg(windows)]
const WINDOWS_STARTUP_APPROVED_KEY: &str =
    "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run";
#[cfg(windows)]
const WINDOWS_STARTUP_VALUE: &str = "Naphwiki Timeline";
#[cfg(windows)]
const WINDOWS_STARTUP_ENABLED: [u8; 12] = [0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
const MAX_EVENT_PAYLOAD_BYTES: usize = 4 * 1024;
const MAX_USERNAME_CHARS: usize = 64;

#[derive(Clone)]
struct WindowTracking(Arc<Mutex<WindowTrackingSettings>>);

struct WindowTrackingSettings {
    always_on_top: bool,
    hide_when_unfocused: bool,
    #[cfg(windows)]
    exclude_from_capture: bool,
    #[cfg(windows)]
    preferred_process: String,
    #[cfg(windows)]
    attached_process: Option<String>,
    #[cfg(windows)]
    target: Option<AttachedWindow>,
    #[cfg(windows)]
    selection_prompt_open: bool,
    #[cfg(windows)]
    selection_armed: bool,
    #[cfg(windows)]
    actual_topmost: Option<bool>,
    #[cfg(windows)]
    remembered_offset: Option<(i32, i32)>,
    #[cfg(windows)]
    persistence_path: Option<PathBuf>,
    #[cfg(windows)]
    background_mode: bool,
}

#[cfg(windows)]
struct AttachedWindow {
    handle: isize,
    process_id: u32,
    offset: (i32, i32),
    last_target_position: (i32, i32),
    last_overlay_position: (i32, i32),
}

impl Default for WindowTracking {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(WindowTrackingSettings {
            always_on_top: true,
            hide_when_unfocused: true,
            #[cfg(windows)]
            exclude_from_capture: false,
            #[cfg(windows)]
            preferred_process: DEFAULT_TARGET_PROCESS.to_string(),
            #[cfg(windows)]
            attached_process: None,
            #[cfg(windows)]
            target: None,
            #[cfg(windows)]
            selection_prompt_open: false,
            #[cfg(windows)]
            selection_armed: false,
            #[cfg(windows)]
            actual_topmost: None,
            #[cfg(windows)]
            remembered_offset: None,
            #[cfg(windows)]
            persistence_path: None,
            #[cfg(windows)]
            background_mode: false,
        })))
    }
}

impl WindowTracking {
    fn lock(&self) -> MutexGuard<'_, WindowTrackingSettings> {
        self.0.lock().unwrap_or_else(|error| error.into_inner())
    }

    #[cfg(windows)]
    fn configure(&self, persistence_path: Option<PathBuf>, background_mode: bool) {
        let persisted = persistence_path.as_deref().and_then(load_tracking_settings);
        let mut settings = self.lock();
        if let Some(persisted) = persisted {
            if is_valid_process_name(&persisted.preferred_process) {
                settings.preferred_process = persisted.preferred_process;
            }
            settings.remembered_offset = persisted.offset.map(|offset| (offset[0], offset[1]));
            settings.exclude_from_capture = persisted.exclude_from_capture;
        }
        settings.persistence_path = persistence_path;
        settings.background_mode = background_mode;
    }
}

#[cfg(windows)]
#[derive(serde::Deserialize, serde::Serialize)]
struct PersistedWindowTracking {
    preferred_process: String,
    offset: Option<[i32; 2]>,
    #[serde(default)]
    exclude_from_capture: bool,
}

#[cfg(windows)]
fn load_tracking_settings(path: &Path) -> Option<PersistedWindowTracking> {
    if std::fs::metadata(path).ok()?.len() > MAX_TRACKING_SETTINGS_BYTES {
        return None;
    }
    let contents = std::fs::read(path).ok()?;
    serde_json::from_slice(&contents).ok()
}

#[cfg(windows)]
fn persist_tracking_settings(settings: &WindowTrackingSettings) {
    let Some(path) = settings.persistence_path.as_deref() else {
        return;
    };
    let persisted = PersistedWindowTracking {
        preferred_process: settings.preferred_process.clone(),
        offset: settings.remembered_offset.map(|(x, y)| [x, y]),
        exclude_from_capture: settings.exclude_from_capture,
    };
    let Ok(contents) = serde_json::to_vec_pretty(&persisted) else {
        return;
    };
    if let Some(parent) = path.parent() {
        if std::fs::create_dir_all(parent).is_err() {
            return;
        }
    }
    let _ = std::fs::write(path, contents);
}

#[cfg(windows)]
fn is_valid_process_name(process: &str) -> bool {
    !process.trim().is_empty()
        && process.len() <= 260
        && !process.chars().any(|character| character.is_control())
}

#[cfg(windows)]
fn launched_by_autostart() -> bool {
    std::env::args_os().any(|argument| argument == std::ffi::OsStr::new(AUTOSTART_ARGUMENT))
}

#[cfg(windows)]
fn windows_startup_command() -> std::io::Result<String> {
    let executable = std::env::current_exe()?;
    Ok(format!(
        "\"{}\" {}",
        executable.display(),
        AUTOSTART_ARGUMENT
    ))
}

#[cfg(windows)]
fn windows_startup_enabled() -> std::io::Result<bool> {
    let current_user = RegKey::predef(HKEY_CURRENT_USER);
    let run_key = current_user.open_subkey_with_flags(WINDOWS_RUN_KEY, KEY_READ)?;
    let registered_command = run_key.get_value::<String, _>(WINDOWS_STARTUP_VALUE).ok();
    let expected_command = windows_startup_command()?;
    if registered_command.as_deref() != Some(expected_command.as_str()) {
        return Ok(false);
    }

    let task_manager_enabled = current_user
        .open_subkey_with_flags(WINDOWS_STARTUP_APPROVED_KEY, KEY_READ)
        .ok()
        .and_then(|key| key.get_raw_value(WINDOWS_STARTUP_VALUE).ok())
        .and_then(|value| {
            (value.bytes.len() >= 8)
                .then(|| value.bytes.iter().rev().take(8).all(|byte| *byte == 0))
        })
        .unwrap_or(true);
    Ok(task_manager_enabled)
}

#[cfg(windows)]
fn set_windows_startup(enabled: bool) -> std::io::Result<()> {
    let current_user = RegKey::predef(HKEY_CURRENT_USER);
    let run_key = current_user.open_subkey_with_flags(WINDOWS_RUN_KEY, KEY_SET_VALUE)?;
    if enabled {
        run_key.set_value(WINDOWS_STARTUP_VALUE, &windows_startup_command()?)?;
        if let Ok(startup_approved) =
            current_user.open_subkey_with_flags(WINDOWS_STARTUP_APPROVED_KEY, KEY_SET_VALUE)
        {
            startup_approved.set_raw_value(
                WINDOWS_STARTUP_VALUE,
                &RegValue {
                    vtype: REG_BINARY,
                    bytes: WINDOWS_STARTUP_ENABLED.to_vec(),
                },
            )?;
        }
    } else {
        match run_key.delete_value(WINDOWS_STARTUP_VALUE) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        if let Ok(startup_approved) =
            current_user.open_subkey_with_flags(WINDOWS_STARTUP_APPROVED_KEY, KEY_SET_VALUE)
        {
            match startup_approved.delete_value(WINDOWS_STARTUP_VALUE) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
    }
    Ok(())
}

/// Adds native window behavior to the live timeline page. Content is not
/// selectable, left mouse presses outside interactive controls drag the app
/// window, and the built-in context menu is replaced by native settings.
///
/// The settings menu looks up the current auth state from the same-origin site,
/// so the session cookie is sent. `GET /api/me` must return an object with a
/// `user` property. The property is null while logged out and contains the user
/// object while logged in. Failed, malformed, and timed out requests use the
/// logged-out menu.
const WINDOW_INTEGRATION_SCRIPT: &str = r#"
(function () {
  var DRAG_EXCLUSION_SELECTOR = [
    'a',
    'button',
    'input',
    'select',
    'textarea',
    'label',
    '[contenteditable="true"]',
    '[role="button"]',
    '[role="link"]',
    '[data-tauri-drag-region]',
    '.embed-resize'
  ].join(',');

  function installSelectionStyles() {
    if (document.getElementById('naphwiki-window-interaction-styles')) return;
    var style = document.createElement('style');
    style.id = 'naphwiki-window-interaction-styles';
    style.textContent = [
      'html, body, body * {',
      '  -webkit-user-select: none !important;',
      '  user-select: none !important;',
      '}',
      'input, textarea, [contenteditable="true"] {',
      '  -webkit-user-select: text !important;',
      '  user-select: text !important;',
      '}'
    ].join('\n');
    (document.head || document.documentElement).appendChild(style);
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', installSelectionStyles, { once: true });
  } else {
    installSelectionStyles();
  }

  window.addEventListener('mousedown', function (e) {
    if (e.button !== 0 || !window.__TAURI__) return;
    var target = e.target instanceof Element ? e.target : null;
    if (target && target.closest(DRAG_EXCLUSION_SELECTOR)) return;
    e.preventDefault();
    window.__TAURI__.window.getCurrentWindow().startDragging()
      .catch(function () {});
  }, true);

  async function authState() {
    var fallback = { loggedIn: false, username: null };
    var controller = new AbortController();
    var timeout = setTimeout(function () { controller.abort(); }, 1500);
    try {
      var res = await fetch('/api/me', {
        credentials: 'same-origin',
        signal: controller.signal
      });
      if (!res.ok) return fallback;
      var me = await res.json();
      var user = me && typeof me === 'object' ? me.user : null;
      if (!user || typeof user !== 'object') return fallback;
      var username = user.username || user.displayName || user.display_name
        || user.globalName || user.global_name || user.name || null;
      return {
        loggedIn: true,
        username: typeof username === 'string' ? username : null
      };
    } catch (_) {
      return fallback;
    } finally {
      clearTimeout(timeout);
    }
  }
  window.addEventListener('contextmenu', function (e) {
    e.preventDefault();
    if (!window.__TAURI__) return;
    authState().then(function (state) {
      window.__TAURI__.event.emit('timeline-context-menu', state)
        .catch(function () {});
    });
  }, true);
})();
"#;

#[derive(Default, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct AuthState {
    logged_in: bool,
    username: Option<String>,
}

#[derive(Default, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct OrientationState {
    vertical: bool,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(windows)]
    let builder =
        tauri::Builder::default().plugin(tauri_plugin_single_instance::init(|app, args, _| {
            if args
                .iter()
                .any(|argument| argument.as_str() == AUTOSTART_ARGUMENT)
            {
                return;
            }
            app.state::<WindowTracking>().lock().background_mode = false;
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
            let _ = show_version_history_once(app);
        }));
    #[cfg(not(windows))]
    let builder = tauri::Builder::default();

    #[cfg(windows)]
    let background_mode = launched_by_autostart();

    builder
        .manage(WindowTracking::default())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(move |app| {
            #[cfg(windows)]
            {
                let persistence_path = app
                    .path()
                    .app_config_dir()
                    .ok()
                    .map(|path| path.join(TRACKING_SETTINGS_FILE));
                app.state::<WindowTracking>()
                    .configure(persistence_path, background_mode);
            }

            // The main window has `create: false` in tauri.conf.json so it can
            // be built here with the window integration script attached.
            let mut window_config = app
                .config()
                .app
                .windows
                .iter()
                .find(|w| w.label == "main")
                .expect("main window missing from tauri.conf.json")
                .clone();
            #[cfg(windows)]
            if background_mode {
                window_config.visible = false;
            }
            let main = tauri::WebviewWindowBuilder::from_config(app.handle(), &window_config)?
                .initialization_script(WINDOW_INTEGRATION_SCRIPT)
                .build()?;

            #[cfg(windows)]
            {
                let native_window = main.hwnd()?.0 as isize;
                let exclude_from_capture =
                    app.state::<WindowTracking>().lock().exclude_from_capture;
                if exclude_from_capture
                    && window_tracking::capture_exclusion_supported()
                    && !window_tracking::set_capture_exclusion(native_window, true)
                {
                    let tracking = app.state::<WindowTracking>();
                    let mut settings = tracking.lock();
                    settings.exclude_from_capture = false;
                    persist_tracking_settings(&settings);
                }
                window_tracking::start(
                    native_window,
                    app.state::<WindowTracking>().inner().clone(),
                );
            }

            let handle = app.handle().clone();
            main.listen(CONTEXT_MENU_EVENT, move |event| {
                let auth = if event.payload().len() <= MAX_EVENT_PAYLOAD_BYTES {
                    serde_json::from_str::<AuthState>(event.payload()).unwrap_or_default()
                } else {
                    AuthState::default()
                };
                let handle = handle.clone();
                let main_thread_handle = handle.clone();
                let _ = handle
                    .run_on_main_thread(move || show_context_menu(&main_thread_handle, &auth));
            });

            let orientation_handle = app.handle().clone();
            main.listen(ORIENTATION_EVENT, move |event| {
                if event.payload().len() > MAX_EVENT_PAYLOAD_BYTES {
                    return;
                }
                let orientation =
                    serde_json::from_str::<OrientationState>(event.payload()).unwrap_or_default();
                let handle = orientation_handle.clone();
                let _ = orientation_handle.run_on_main_thread(move || {
                    let Some(window) = handle.get_webview_window("main") else {
                        return;
                    };
                    let Ok(scale_factor) = window.scale_factor() else {
                        return;
                    };
                    let Ok(size) = window.inner_size() else {
                        return;
                    };
                    let logical = size.to_logical::<f64>(scale_factor);

                    if orientation.vertical {
                        let _ = window.set_min_size(Some(tauri::LogicalSize::new(20.0, 320.0)));
                        if logical.width > logical.height {
                            let _ = window
                                .set_size(tauri::LogicalSize::new(240.0, logical.width.max(320.0)));
                        }
                    } else {
                        let _ = window.set_min_size(Some(tauri::LogicalSize::new(320.0, 20.0)));
                        if logical.height > logical.width {
                            let _ = window
                                .set_size(tauri::LogicalSize::new(logical.height.max(320.0), 75.0));
                        }
                    }
                });
            });

            let settings_refresh_generation = Arc::new(AtomicU64::new(0));
            let settings_refresh_handle = app.handle().clone();
            app.listen(SETTINGS_CHANGE_EVENT, move |_| {
                let generation = settings_refresh_generation.fetch_add(1, Ordering::SeqCst) + 1;
                let latest_generation = settings_refresh_generation.clone();
                let handle = settings_refresh_handle.clone();
                thread::spawn(move || {
                    thread::sleep(Duration::from_secs(2));
                    if latest_generation.load(Ordering::SeqCst) != generation {
                        return;
                    }
                    let main_thread_handle = handle.clone();
                    let _ = handle.run_on_main_thread(move || {
                        if let Some(main) = main_thread_handle.get_webview_window("main") {
                            let _ = main.reload();
                        }
                    });
                });
            });

            let launch_handle = app.handle().clone();
            #[cfg(windows)]
            let show_version_history = !background_mode;
            #[cfg(not(windows))]
            let show_version_history = true;
            let version_history_open =
                show_version_history && show_version_history_once(app.handle());
            let _ = thread::Builder::new()
                .name("launch-notifications".to_string())
                .spawn(move || {
                    while version_history_open
                        && launch_handle
                            .get_webview_window(VERSION_HISTORY_WINDOW)
                            .is_some()
                    {
                        thread::sleep(Duration::from_millis(250));
                    }
                    tauri::async_runtime::block_on(check_for_updates(launch_handle));
                });
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "timeline-settings" {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                    let app = window.app_handle().clone();
                    thread::spawn(move || {
                        thread::sleep(Duration::from_secs(2));
                        if let Some(settings) = app.get_webview_window("timeline-settings") {
                            let _ = settings.destroy();
                        }
                        if let Some(main) = app.get_webview_window("main") {
                            let _ = main.reload();
                        }
                    });
                    return;
                }
            }

            // The strip refreshes itself when the login popup it opened goes
            // away; when the popup was opened from the context menu instead,
            // trigger that refresh here.
            if matches!(event, WindowEvent::Destroyed) {
                if window.label() == "discord-login" {
                    if let Some(main) = window.app_handle().get_webview_window("main") {
                        let _ = main.reload();
                    }
                }
            }
        })
        .on_menu_event(|app, event| match event.id().as_ref() {
            MENU_LOGIN => open_login_window(app),
            MENU_LOGOUT => log_out(app),
            MENU_TIMELINE_SETTINGS => open_settings_window(app),
            MENU_ALWAYS_ON_TOP => toggle_always_on_top(app),
            MENU_HIDE_WHEN_UNFOCUSED => toggle_hide_when_unfocused(app),
            #[cfg(windows)]
            MENU_EXCLUDE_FROM_CAPTURE => toggle_exclude_from_capture(app),
            MENU_ATTACH_WINDOW => request_window_selection(app),
            #[cfg(windows)]
            MENU_START_WITH_WINDOWS => toggle_start_with_windows(app),
            MENU_OPEN_SITE => {
                let _ = app.opener().open_url(SITE_URL, None::<&str>);
            }
            MENU_BECOME_PREMIUM => {
                let _ = app.opener().open_url(PATREON_MEMBERSHIP_URL, None::<&str>);
            }
            MENU_CLOSE => app.exit(0),
            _ => {}
        })
        .run(tauri::generate_context!())
        .expect("error while running naphwiki timeline");
}

fn show_version_history_once(app: &AppHandle) -> bool {
    let version = app.package_info().version.to_string();
    let Ok(config_dir) = app.path().app_config_dir() else {
        return false;
    };
    let state_path = config_dir.join(CHANGELOG_STATE_FILE);
    let already_seen = std::fs::read_to_string(&state_path)
        .ok()
        .is_some_and(|last_seen| last_seen.trim() == version.as_str());
    if already_seen {
        return false;
    }

    let Ok(url) = "about:blank".parse::<tauri::Url>() else {
        return false;
    };
    let Ok(document) = serde_json::to_string(VERSION_HISTORY_HTML) else {
        return false;
    };
    let initialization_script = format!(
        "document.addEventListener('DOMContentLoaded',function(){{document.open();document.write({document});document.close();}},{{once:true}});"
    );
    let window =
        tauri::WebviewWindowBuilder::new(app, VERSION_HISTORY_WINDOW, WebviewUrl::External(url))
            .title(format!("Naphwiki Timeline {version} - version history"))
            .inner_size(520.0, 600.0)
            .min_inner_size(400.0, 340.0)
            .center()
            .resizable(true)
            .always_on_top(true)
            .visible(false)
            .initialization_script(initialization_script)
            .on_page_load(|window, payload| {
                if matches!(payload.event(), tauri::webview::PageLoadEvent::Finished) {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            })
            .build();
    if window.is_err() {
        return false;
    }

    if std::fs::create_dir_all(&config_dir).is_ok() {
        let _ = std::fs::write(state_path, version);
    }
    true
}

async fn check_for_updates(app: AppHandle) {
    let updater = match app.updater() {
        Ok(updater) => updater,
        Err(_) => return,
    };
    let update = match updater.check().await {
        Ok(Some(update)) => update,
        Ok(None) | Err(_) => return,
    };
    let version = update.version.clone();
    let response = app
        .dialog()
        .message(format!(
            "Naphwiki Timeline {version} is available.\n\nDownload and install it now?"
        ))
        .title("Update available")
        .kind(MessageDialogKind::Info)
        .buttons(MessageDialogButtons::YesNo)
        .blocking_show();

    if !response {
        return;
    }

    if update.download_and_install(|_, _| {}, || {}).await.is_err() {
        app.dialog()
            .message("The update could not be downloaded or installed. Please try again later.")
            .title("Update failed")
            .kind(MessageDialogKind::Error)
            .blocking_show();
        return;
    }

    app.restart();
}

fn show_context_menu(app: &AppHandle, auth: &AuthState) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let tracking = app.state::<WindowTracking>();
    let on_top = tracking.lock().always_on_top;
    #[cfg(windows)]
    let (
        hide_when_unfocused,
        exclude_from_capture,
        capture_exclusion_supported,
        tracking_status,
        start_with_windows,
    ) = {
        let settings = tracking.lock();
        let capture_exclusion_supported = window_tracking::capture_exclusion_supported();
        (
            settings.hide_when_unfocused,
            settings.exclude_from_capture && capture_exclusion_supported,
            capture_exclusion_supported,
            tracking_status_label(&settings),
            windows_startup_enabled().unwrap_or(false),
        )
    };
    let menu = (|| -> tauri::Result<Menu<Wry>> {
        let menu = Menu::new(app)?;
        let account_menu = Submenu::new(app, "Account", true)?;
        if auth.logged_in {
            let label = match normalized_username(auth.username.as_deref()) {
                Some(name) => format!("Logged in as {name}"),
                None => "Logged in".to_string(),
            };
            account_menu.append(&MenuItem::with_id(
                app,
                MENU_LOGGED_IN_AS,
                label,
                false,
                None::<&str>,
            )?)?;
            account_menu.append(&PredefinedMenuItem::separator(app)?)?;
            account_menu.append(&MenuItem::with_id(
                app,
                MENU_TIMELINE_SETTINGS,
                "Timeline settings",
                true,
                None::<&str>,
            )?)?;
            account_menu.append(&MenuItem::with_id(
                app,
                MENU_LOGOUT,
                "Log out",
                true,
                None::<&str>,
            )?)?;
        } else {
            account_menu.append(&MenuItem::with_id(
                app,
                MENU_LOGIN,
                "Login",
                true,
                None::<&str>,
            )?)?;
        }
        menu.append(&account_menu)?;

        #[cfg(windows)]
        {
            let tracking_menu = Submenu::new(app, "Window tracking", true)?;
            tracking_menu.append(&MenuItem::with_id(
                app,
                MENU_ATTACH_WINDOW,
                "Attach to window",
                true,
                None::<&str>,
            )?)?;
            tracking_menu.append(&MenuItem::with_id(
                app,
                MENU_ATTACHED_PROCESS,
                tracking_status,
                false,
                None::<&str>,
            )?)?;
            tracking_menu.append(&PredefinedMenuItem::separator(app)?)?;
            tracking_menu.append(&CheckMenuItem::with_id(
                app,
                MENU_HIDE_WHEN_UNFOCUSED,
                "Hide when game is not in focus",
                true,
                hide_when_unfocused,
                None::<&str>,
            )?)?;
            menu.append(&tracking_menu)?;
        }

        let window_menu = Submenu::new(app, "Window behavior", true)?;
        window_menu.append(&CheckMenuItem::with_id(
            app,
            MENU_ALWAYS_ON_TOP,
            "Always on top",
            true,
            on_top,
            None::<&str>,
        )?)?;
        #[cfg(windows)]
        {
            let capture_label = if capture_exclusion_supported {
                "Exclude from capture"
            } else {
                "Exclude from capture (requires Windows 10 2004+)"
            };
            window_menu.append(&CheckMenuItem::with_id(
                app,
                MENU_EXCLUDE_FROM_CAPTURE,
                capture_label,
                capture_exclusion_supported,
                exclude_from_capture,
                None::<&str>,
            )?)?;
        }
        menu.append(&window_menu)?;

        #[cfg(windows)]
        {
            let application_menu = Submenu::new(app, "Application", true)?;
            application_menu.append(&CheckMenuItem::with_id(
                app,
                MENU_START_WITH_WINDOWS,
                "Start with Windows",
                true,
                start_with_windows,
                None::<&str>,
            )?)?;
            menu.append(&application_menu)?;
        }
        menu.append(&PredefinedMenuItem::separator(app)?)?;
        menu.append(&MenuItem::with_id(
            app,
            MENU_OPEN_SITE,
            "Go to Naphwiki.com",
            true,
            None::<&str>,
        )?)?;
        menu.append(&MenuItem::with_id(
            app,
            MENU_BECOME_PREMIUM,
            "Become a premium supporter",
            true,
            None::<&str>,
        )?)?;
        menu.append(&PredefinedMenuItem::separator(app)?)?;
        menu.append(&MenuItem::with_id(
            app,
            MENU_CLOSE,
            "Close",
            true,
            None::<&str>,
        )?)?;
        Ok(menu)
    })();
    if let Ok(menu) = menu {
        let _ = window.popup_menu(&menu);
    }
}

fn toggle_always_on_top(app: &AppHandle) {
    let tracking = app.state::<WindowTracking>();
    let always_on_top = {
        let mut settings = tracking.lock();
        settings.always_on_top = !settings.always_on_top;
        #[cfg(windows)]
        {
            settings.actual_topmost = None;
        }
        settings.always_on_top
    };

    #[cfg(not(windows))]
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.set_always_on_top(always_on_top);
    }

    #[cfg(windows)]
    let _ = always_on_top;
}

fn toggle_hide_when_unfocused(app: &AppHandle) {
    let tracking = app.state::<WindowTracking>();
    let mut settings = tracking.lock();
    settings.hide_when_unfocused = !settings.hide_when_unfocused;
    #[cfg(windows)]
    {
        settings.actual_topmost = None;
    }
}

#[cfg(windows)]
fn toggle_exclude_from_capture(app: &AppHandle) {
    if !window_tracking::capture_exclusion_supported() {
        return;
    }
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let Ok(native_window) = window.hwnd() else {
        return;
    };
    let tracking = app.state::<WindowTracking>();
    let exclude_from_capture = !tracking.lock().exclude_from_capture;
    if window_tracking::set_capture_exclusion(native_window.0 as isize, exclude_from_capture) {
        let mut settings = tracking.lock();
        settings.exclude_from_capture = exclude_from_capture;
        persist_tracking_settings(&settings);
    } else {
        app.dialog()
            .message("The capture exclusion setting could not be changed.")
            .title("Capture setting failed")
            .kind(MessageDialogKind::Error)
            .blocking_show();
    }
}

#[cfg(windows)]
fn toggle_start_with_windows(app: &AppHandle) {
    let result = windows_startup_enabled().and_then(|enabled| set_windows_startup(!enabled));
    if result.is_err() {
        app.dialog()
            .message("The Windows startup setting could not be changed.")
            .title("Startup setting failed")
            .kind(MessageDialogKind::Error)
            .blocking_show();
    }
}

fn request_window_selection(app: &AppHandle) {
    #[cfg(windows)]
    if let Some(window) = app.get_webview_window("main") {
        if let Ok(native_window) = window.hwnd() {
            window_tracking::request_selection(
                native_window.0 as isize,
                app.state::<WindowTracking>().inner().clone(),
            );
        }
    }

    #[cfg(not(windows))]
    let _ = app;
}

#[cfg(windows)]
fn tracking_status_label(settings: &WindowTrackingSettings) -> String {
    if settings.selection_prompt_open {
        return "Choose a window in the open prompt".to_string();
    }
    if settings.selection_armed {
        return "Click a window to attach".to_string();
    }
    match settings.attached_process.as_deref() {
        Some(process) => format!("Attached to: {process}"),
        None => format!("Looking for: {}", settings.preferred_process),
    }
}

#[cfg_attr(not(windows), allow(dead_code))]
fn effective_topmost(
    always_on_top: bool,
    hide_when_unfocused: bool,
    target_is_focused: bool,
) -> bool {
    always_on_top && (!hide_when_unfocused || target_is_focused)
}

/// Opens the same decorated Discord login popup the strip's own hover button
/// uses (label `discord-login`, so the site closes it after the OAuth flow
/// lands back on /timeline).
fn open_login_window(app: &AppHandle) {
    if let Some(existing) = app.get_webview_window("discord-login") {
        let _ = existing.show();
        let _ = existing.set_focus();
        return;
    }
    let url: tauri::Url = match LOGIN_URL.parse() {
        Ok(url) => url,
        Err(_) => return,
    };
    let _ = tauri::WebviewWindowBuilder::new(app, "discord-login", WebviewUrl::External(url))
        .title("Log in with Discord")
        .inner_size(520.0, 780.0)
        .center()
        .always_on_top(true)
        .build();
}

fn open_settings_window(app: &AppHandle) {
    if let Some(existing) = app.get_webview_window("timeline-settings") {
        if existing.is_visible().unwrap_or(false) {
            let _ = existing.set_focus();
        }
        return;
    }
    let url: tauri::Url = match SETTINGS_URL.parse() {
        Ok(url) => url,
        Err(_) => return,
    };
    let _ = tauri::WebviewWindowBuilder::new(app, "timeline-settings", WebviewUrl::External(url))
        .title("Timeline settings")
        .inner_size(760.0, 820.0)
        .min_inner_size(480.0, 520.0)
        .center()
        .resizable(true)
        .build();
}

/// Logs out locally: drops the webview's browsing data (which holds the
/// session cookie) and reloads the strip so it renders logged out.
fn log_out(app: &AppHandle) {
    if let Some(main) = app.get_webview_window("main") {
        if main.clear_all_browsing_data().is_ok() {
            let _ = main.reload();
        }
    }
}

fn normalized_username(username: Option<&str>) -> Option<String> {
    let clean = username?
        .chars()
        .filter(|character| !character.is_control())
        .take(MAX_USERNAME_CHARS)
        .collect::<String>();
    let clean = clean.trim();
    (!clean.is_empty()).then(|| clean.to_string())
}

#[cfg(test)]
mod tests {
    use super::{effective_topmost, normalized_username, MAX_USERNAME_CHARS};
    #[cfg(windows)]
    use super::{
        is_valid_process_name, windows_startup_command, PersistedWindowTracking, AUTOSTART_ARGUMENT,
    };

    #[test]
    fn username_is_trimmed_and_control_characters_are_removed() {
        assert_eq!(
            normalized_username(Some("  Timeline\nUser\0  ")),
            Some("TimelineUser".to_string())
        );
    }

    #[test]
    fn empty_username_is_omitted() {
        assert_eq!(normalized_username(Some(" \n\0 ")), None);
        assert_eq!(normalized_username(None), None);
    }

    #[test]
    fn username_length_is_limited() {
        let username = "a".repeat(MAX_USERNAME_CHARS + 10);
        assert_eq!(
            normalized_username(Some(&username)),
            Some("a".repeat(MAX_USERNAME_CHARS))
        );
    }

    #[test]
    fn focus_setting_only_suppresses_always_on_top_while_unfocused() {
        assert!(effective_topmost(true, true, true));
        assert!(!effective_topmost(true, true, false));
        assert!(effective_topmost(true, false, false));
        assert!(!effective_topmost(false, false, true));
        assert!(!effective_topmost(false, true, true));
    }

    #[cfg(windows)]
    #[test]
    fn remembered_process_name_must_be_safe_and_nonempty() {
        assert!(is_valid_process_name("L2.bin"));
        assert!(!is_valid_process_name(""));
        assert!(!is_valid_process_name("L2.bin\n"));
        assert!(!is_valid_process_name(&"a".repeat(261)));
    }

    #[cfg(windows)]
    #[test]
    fn startup_command_quotes_the_executable_path() {
        let command = windows_startup_command().expect("startup command");
        let suffix = format!("\" {AUTOSTART_ARGUMENT}");
        assert!(command.starts_with('"'));
        assert!(command.ends_with(suffix.as_str()));
    }

    #[cfg(windows)]
    #[test]
    fn remembered_attachment_serializes_with_its_offset() {
        let settings = PersistedWindowTracking {
            preferred_process: "L2.bin".to_string(),
            offset: Some([48, -12]),
            exclude_from_capture: true,
        };
        let serialized = serde_json::to_vec(&settings).expect("serialize settings");
        let restored: PersistedWindowTracking =
            serde_json::from_slice(&serialized).expect("restore settings");
        assert_eq!(restored.preferred_process, "L2.bin");
        assert_eq!(restored.offset, Some([48, -12]));
        assert!(restored.exclude_from_capture);
    }

    #[cfg(windows)]
    #[test]
    fn older_tracking_settings_default_capture_exclusion_to_off() {
        let restored: PersistedWindowTracking =
            serde_json::from_str(r#"{"preferred_process":"L2.bin","offset":[48,-12]}"#)
                .expect("restore old settings");
        assert!(!restored.exclude_from_capture);
    }
}
