#[cfg(windows)]
use std::path::{Path, PathBuf};
use std::{
    sync::{Arc, Mutex, MutexGuard},
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
const CONTEXT_MENU_EVENT: &str = "timeline-context-menu";
const ORIENTATION_EVENT: &str = "timeline-orientation-change";

const SITE_URL: &str = "https://www.naphwiki.com";
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
#[serde(default)]
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
                        let _ = window.set_min_size(Some(tauri::LogicalSize::new(160.0, 320.0)));
                        if logical.width > logical.height {
                            let _ = window
                                .set_size(tauri::LogicalSize::new(240.0, logical.width.max(320.0)));
                        }
                    } else {
                        let _ = window.set_min_size(Some(tauri::LogicalSize::new(320.0, 25.0)));
                        if logical.height > logical.width {
                            let _ = window
                                .set_size(tauri::LogicalSize::new(logical.height.max(320.0), 75.0));
                        }
                    }
                });
            });

            let update_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                check_for_updates(update_handle).await;
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
            _ => {}
        })
        .run(tauri::generate_context!())
        .expect("error while running naphwiki timeline");
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
