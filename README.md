# Naphwiki Timeline

The [Naphwiki](https://www.naphwiki.com) event timeline as a thin, frameless,
always-on-top desktop strip, meant to sit above the game so you can glance at
upcoming events without alt-tabbing.

The app is a **Tauri v2** shell: a small Rust binary that opens a single
undecorated webview window pointed at the public
[`https://www.naphwiki.com/timeline`](https://www.naphwiki.com/timeline) page.
All UI, including the event bar, clock row, tooltips, close button, and resize
handles, is rendered by the website itself (its embed mode detects the Tauri
webview and adds the window chrome). Because the frontend is loaded remotely,
the app picks up every site deploy automatically and never needs a re-release
for content changes.

This is a shell that loads the public website. It contains **no bundled
credentials or private infrastructure**. Authentication is handled by the
public website. The app stores no data of its own beyond the webview's regular
cookies and local storage, which are kept in your OS user profile like browser
data.

## Features

- Frameless strip, about 900 by 96 pixels, always on top; resize it wider to
  see more hours (the page scales its time window with width automatically).
- Hold the left mouse button on the timeline background or an event box to
  move the window. Hover effects remain active, and interactive controls keep
  their normal behavior. Invisible edge/corner handles resize the window
  (undecorated windows have no native borders on macOS/most Linux).
- A close button in the top-right corner closes the app.
- Right-click anywhere in the strip for a settings menu: **Login** while
  logged out (or **Logged in as _username_** plus **Log out** while logged
  in), toggle **Always on Top**, or **Go to Naphwiki.com** to open the full
  site in your default browser.
- Optional Discord login (hover-reveal button, top-left, while logged out):
  opens a normal-sized login window, and your customized timeline (hidden
  events, custom events, notification settings) loads into the strip once it
  completes. The session persists across restarts in the webview profile.

## Install

Grab the bundle for your platform from the
[Releases](../../releases) page:

- **Windows**: `.msi` or NSIS `.exe`
- **macOS**: `.dmg`
- **Linux**: `.deb`, `.rpm`, or AppImage

Builds are currently unsigned, so your OS may show a warning on first launch.

## Development

Prerequisites:

- Rust stable - <https://rustup.rs>
- Tauri CLI: `cargo install tauri-cli --version "^2"`
- Platform webview deps: WebView2 on Windows (usually preinstalled),
  `libwebkit2gtk-4.1-dev` (plus `libgtk-3-dev`, `librsvg2-dev`,
  `libayatana-appindicator3-dev` as needed) on Linux, nothing extra on macOS.

Two ways to run during development:

1. **Against local site code** (when developing the embed page itself): run
   `npm run dev` in the naphwiki repo (Vite on `:5173`), then `cargo tauri dev`
   here. The window loads `http://localhost:5173/timeline` with hot reload.
2. **Against production** (when only touching the shell): temporarily set
   `build.devUrl` in `src-tauri/tauri.conf.json` to
   `https://www.naphwiki.com/timeline` and `cargo tauri dev`, or just
   `cargo tauri build` and run the bundle.

Production builds: `cargo tauri build` produces platform bundles under
`src-tauri/target/release/bundle/`.

### Icons

`src-tauri/icons/` is generated from a 512×512 logo PNG with:

```
cargo tauri icon path/to/naphwiki-512.png
```

Re-run that command to regenerate the full set after changing the logo.

## How it works

- `src-tauri/tauri.conf.json` points `frontendDist` at
  `https://www.naphwiki.com/timeline` and configures the single frameless,
  resizable, always-on-top window. `withGlobalTauri: true` injects
  `window.__TAURI__`, which is how the site knows to render its embed chrome
  (close button, resize handles, Tauri-aware login popup).
- The right-click settings menu is native: a small initialization script
  injected into the main window (see `src-tauri/src/lib.rs`) suppresses the
  webview's default context menu, looks up the current auth state, and emits
  an event; the Rust side pops up a native menu at the cursor. "Go to
  Naphwiki.com" opens the default browser via `tauri-plugin-opener`, which is
  used only from Rust. The page gets no opener permission.
- The same initialization script disables content selection and starts native
  window dragging on left mouse presses outside links, form fields, buttons,
  and resize handles. It does not add an overlay, so timeline hover effects
  continue to receive normal pointer movement.
- The menu's auth section expects `GET /api/me` to return a `user` object when
  logged in and a null `user` value when logged out. Failed, malformed, and
  timed out requests fall back to showing "Login". Menu login opens the same
  `discord-login` popup as the strip's hover button. "Log out" clears the
  webview's browsing data and reloads the strip.
- `src-tauri/capabilities/timeline.json` is the whole security story: remote
  origins get **no** Tauri IPC unless a capability grants it. The
  `https://www.naphwiki.com` origin receives only the event, window, and
  webview operations needed by the timeline controls and login popup. The
  separate login capability allows the callback page to close only its own
  window. The page cannot spawn processes, read files, or use unrelated native
  APIs. While the login popup is on `discord.com`, it has no IPC because that
  origin is not allowed.
- Discord OAuth happens entirely on the naphwiki server; the popup is a plain
  webview navigation to the public site, and the session cookie lands in your
  local webview profile, the same place a browser would keep it.

## Notes & limitations

- The webview has its **own cookie jar** and does not share your browser's
  session. Log in once inside the app to sync preferences.
- "Log out" in the context menu is local: it clears the app webview's
  browsing data rather than calling a server-side logout endpoint, so the
  session cookie (and any logged-out localStorage prefs) are dropped on this
  machine only.
- The app shows the live site, so it needs the site deployed and a network
  connection; offline it shows a blank window (v1).
- On Wayland, resize-dragging and always-on-top depend on the compositor.
  X11, Windows, and macOS are fine.

## License

[MIT](LICENSE)
