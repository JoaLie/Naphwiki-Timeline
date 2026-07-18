use tauri::{
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem},
    AppHandle, Listener, Manager, WebviewUrl, WindowEvent, Wry,
};
use tauri_plugin_opener::OpenerExt;

const MENU_LOGIN: &str = "login";
const MENU_LOGOUT: &str = "logout";
const MENU_LOGGED_IN_AS: &str = "logged-in-as";
const MENU_ALWAYS_ON_TOP: &str = "always-on-top";
const MENU_OPEN_SITE: &str = "open-naphwiki";
const CONTEXT_MENU_EVENT: &str = "timeline-context-menu";

const SITE_URL: &str = "https://www.naphwiki.com";
const LOGIN_URL: &str = "https://www.naphwiki.com/auth/discord?returnTo=%2Ftimeline";
const MAX_EVENT_PAYLOAD_BYTES: usize = 4 * 1024;
const MAX_USERNAME_CHARS: usize = 64;

/// Suppresses the webview's built-in context menu, looks up the current auth
/// state from the site (same-origin, so the session cookie is sent), and asks
/// the Rust side to show the native settings menu.
///
/// Site contract: `GET /api/me` returns an object with a `user` property. The
/// property is null while logged out and contains the user object while logged
/// in. Failed, malformed, and timed out requests use the logged-out menu.
const CONTEXT_MENU_SCRIPT: &str = r#"
(function () {
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
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // The main window has `create: false` in tauri.conf.json so it can
            // be built here with the context-menu init script attached.
            let window_config = app
                .config()
                .app
                .windows
                .iter()
                .find(|w| w.label == "main")
                .expect("main window missing from tauri.conf.json")
                .clone();
            let main = tauri::WebviewWindowBuilder::from_config(app.handle(), &window_config)?
                .initialization_script(CONTEXT_MENU_SCRIPT)
                .build()?;

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
            MENU_ALWAYS_ON_TOP => {
                if let Some(window) = app.get_webview_window("main") {
                    if let Ok(on_top) = window.is_always_on_top() {
                        let _ = window.set_always_on_top(!on_top);
                    }
                }
            }
            MENU_OPEN_SITE => {
                let _ = app.opener().open_url(SITE_URL, None::<&str>);
            }
            _ => {}
        })
        .run(tauri::generate_context!())
        .expect("error while running naphwiki timeline");
}

fn show_context_menu(app: &AppHandle, auth: &AuthState) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let on_top = window.is_always_on_top().unwrap_or(false);
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
    use super::{normalized_username, MAX_USERNAME_CHARS};

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
}
