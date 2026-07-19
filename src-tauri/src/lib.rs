use std::sync::{Arc, Mutex, MutexGuard};
use tauri::{
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem},
    AppHandle, Listener, Manager, WebviewUrl, WindowEvent, Wry,
};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tauri_plugin_opener::OpenerExt;
use tauri_plugin_updater::UpdaterExt;

#[cfg(windows)]
mod window_tracking;

const MENU_LOGIN: &str = "login";
const MENU_LOGOUT: &str = "logout";
const MENU_LOGGED_IN_AS: &str = "logged-in-as";
const MENU_ALWAYS_ON_TOP: &str = "always-on-top";
const MENU_HIDE_WHEN_UNFOCUSED: &str = "hide-when-unfocused";
const MENU_ATTACH_WINDOW: &str = "attach-window";
#[cfg(windows)]
const MENU_ATTACHED_PROCESS: &str = "attached-process";
const MENU_OPEN_SITE: &str = "open-naphwiki";
const CONTEXT_MENU_EVENT: &str = "timeline-context-menu";

const SITE_URL: &str = "https://www.naphwiki.com";
const LOGIN_URL: &str = "https://www.naphwiki.com/auth/discord?returnTo=%2Ftimeline";
#[cfg(windows)]
const DEFAULT_TARGET_PROCESS: &str = "Lineage II.exe";
const MAX_EVENT_PAYLOAD_BYTES: usize = 4 * 1024;
const MAX_USERNAME_CHARS: usize = 64;

#[derive(Clone)]
struct WindowTracking(Arc<Mutex<WindowTrackingSettings>>);

struct WindowTrackingSettings {
    always_on_top: bool,
    hide_when_unfocused: bool,
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
        })))
    }
}

impl WindowTracking {
    fn lock(&self) -> MutexGuard<'_, WindowTrackingSettings> {
        self.0.lock().unwrap_or_else(|error| error.into_inner())
    }
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(WindowTracking::default())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            // The main window has `create: false` in tauri.conf.json so it can
            // be built here with the window integration script attached.
            let window_config = app
                .config()
                .app
                .windows
                .iter()
                .find(|w| w.label == "main")
                .expect("main window missing from tauri.conf.json")
                .clone();
            let main = tauri::WebviewWindowBuilder::from_config(app.handle(), &window_config)?
                .initialization_script(WINDOW_INTEGRATION_SCRIPT)
                .build()?;

            #[cfg(windows)]
            {
                let native_window = main.hwnd()?.0 as isize;
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

            let update_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                check_for_updates(update_handle).await;
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            // The strip refreshes itself when the login popup it opened goes
            // away; when the popup was opened from the context menu instead,
            // trigger that refresh here.
            if matches!(event, WindowEvent::Destroyed) && window.label() == "discord-login" {
                if let Some(main) = window.app_handle().get_webview_window("main") {
                    let _ = main.reload();
                }
            }
        })
        .on_menu_event(|app, event| match event.id().as_ref() {
            MENU_LOGIN => open_login_window(app),
            MENU_LOGOUT => log_out(app),
            MENU_ALWAYS_ON_TOP => toggle_always_on_top(app),
            MENU_HIDE_WHEN_UNFOCUSED => toggle_hide_when_unfocused(app),
            MENU_ATTACH_WINDOW => request_window_selection(app),
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
    let (hide_when_unfocused, tracking_status) = {
        let settings = tracking.lock();
        (
            settings.hide_when_unfocused,
            tracking_status_label(&settings),
        )
    };
    let menu = (|| -> tauri::Result<Menu<Wry>> {
        let menu = Menu::new(app)?;
        if auth.logged_in {
            let label = match normalized_username(auth.username.as_deref()) {
                Some(name) => format!("Logged in as {name}"),
                None => "Logged in".to_string(),
            };
            menu.append(&MenuItem::with_id(
                app,
                MENU_LOGGED_IN_AS,
                label,
                false,
                None::<&str>,
            )?)?;
            menu.append(&MenuItem::with_id(
                app,
                MENU_LOGOUT,
                "Log out",
                true,
                None::<&str>,
            )?)?;
        } else {
            menu.append(&MenuItem::with_id(
                app,
                MENU_LOGIN,
                "Login",
                true,
                None::<&str>,
            )?)?;
        }
        menu.append(&PredefinedMenuItem::separator(app)?)?;
        #[cfg(windows)]
        {
            menu.append(&MenuItem::with_id(
                app,
                MENU_ATTACH_WINDOW,
                "Attach to window",
                true,
                None::<&str>,
            )?)?;
            menu.append(&MenuItem::with_id(
                app,
                MENU_ATTACHED_PROCESS,
                tracking_status,
                false,
                None::<&str>,
            )?)?;
            menu.append(&CheckMenuItem::with_id(
                app,
                MENU_HIDE_WHEN_UNFOCUSED,
                "Hide when game is not in focus",
                true,
                hide_when_unfocused,
                None::<&str>,
            )?)?;
        }
        menu.append(&CheckMenuItem::with_id(
            app,
            MENU_ALWAYS_ON_TOP,
            "Always on Top",
            true,
            on_top,
            None::<&str>,
        )?)?;
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
}
