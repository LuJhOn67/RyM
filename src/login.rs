//! Google sign-in through a web view (WebView2 on Windows, WebKitGTK on Linux), run in its own
//! process (`--login`). Writes the YouTube cookie header to `cookie.txt` and exits.
//!
//! Builds without the "webview" feature (the GTK 4 app on Linux: GTK 3, which WebKitGTK uses,
//! can't share a process with GTK 4) open the window through the `RyM` program beside them.

#[cfg(feature = "webview")]
use std::cell::Cell;
#[cfg(feature = "webview")]
use std::rc::Rc;
#[cfg(feature = "webview")]
use std::time::{Duration, Instant};

#[cfg(feature = "webview")]
use tao::event::{Event, WindowEvent};
#[cfg(feature = "webview")]
use tao::event_loop::{ControlFlow, EventLoop};
#[cfg(feature = "webview")]
use tao::window::WindowBuilder;
#[cfg(all(feature = "webview", windows))]
use wry::WebViewBuilderExtWindows;
#[cfg(feature = "webview")]
use wry::{PageLoadEvent, WebContext, WebViewBuilder};

#[cfg(feature = "webview")]
const SIGN_IN: &str = "https://accounts.google.com/ServiceLogin?service=youtube&passive=true\
&continue=https%3A%2F%2Fmusic.youtube.com%2F";
#[cfg(feature = "webview")]
const MUSIC: &str = "https://music.youtube.com/";

/// WebView2 normally takes part in Google's device-bound sessions: the cookies only live a few
/// minutes and only this window could renew them, so a copied session dies right after sign-in.
/// With the feature off, Google hands out a regular session, like it does for Firefox.
/// (The first three are wry's defaults, which this setting replaces.)
#[cfg(all(feature = "webview", windows))]
const BROWSER_ARGS: &str = "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection,EnableBoundSessionCredentials,DeviceBoundSessions";

/// After the page finishes loading, give its scripts time to set cookies.
#[cfg(feature = "webview")]
const SETTLE: Duration = Duration::from_secs(4);

/// Cookies YouTube Music needs to treat a request as signed in.
#[cfg(feature = "webview")]
fn signed_in(names: &[&str]) -> bool {
    names.contains(&"SAPISID") && names.contains(&"LOGIN_INFO")
}

#[cfg(not(feature = "webview"))]
pub fn run() -> ! {
    eprintln!("this build has no sign-in window; it uses RyM's");
    std::process::exit(1)
}

#[cfg(feature = "webview")]
pub fn run() -> ! {
    let event_loop = EventLoop::new();
    let window = WindowBuilder::new()
        .with_title("Sign in to YouTube Music")
        .with_inner_size(tao::dpi::LogicalSize::new(520.0, 720.0))
        .build(&event_loop)
        .expect("login window");
    // Fresh profile: the old one holds device-bound session state.
    let mut ctx = WebContext::new(Some(crate::data_dir().join("webview-signin")));

    // Time the last music.youtube.com page finished loading.
    let loaded: Rc<Cell<Option<Instant>>> = Rc::new(Cell::new(None));
    let on_load = loaded.clone();
    let builder = WebViewBuilder::new_with_web_context(&mut ctx);
    #[cfg(windows)]
    let builder = builder.with_additional_browser_args(BROWSER_ARGS);
    let webview = builder
        .with_url(SIGN_IN)
        .with_on_page_load_handler(move |ev, url| {
            if matches!(ev, PageLoadEvent::Finished) && url.starts_with("https://music.youtube.com") {
                on_load.set(Some(Instant::now()));
            }
        })
        .build_for(&window)
        .expect("webview");

    event_loop.run(move |event, _, flow| {
        *flow = ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(500));
        match event {
            // Exit through the event loop (not process::exit) so WebView2 shuts down cleanly
            // and saves the signed-in profile to disk.
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => *flow = ControlFlow::ExitWithCode(1),
            Event::NewEvents(_) => {
                if !loaded.get().is_some_and(|t| t.elapsed() >= SETTLE) {
                    return;
                }
                let Ok(cookies) = webview.cookies_for_url(MUSIC) else { return };
                let names: Vec<&str> = cookies.iter().map(|c| c.name()).collect();
                if !signed_in(&names) {
                    return;
                }
                let header = cookies
                    .iter()
                    .map(|c| format!("{}={}", c.name(), c.value()))
                    .collect::<Vec<_>>()
                    .join("; ");
                let _ = std::fs::create_dir_all(crate::data_dir());
                let ok = std::fs::write(crate::cookie_path(), &header).is_ok();
                *flow = ControlFlow::ExitWithCode(if ok { 0 } else { 1 });
            }
            _ => {}
        }
    })
}

/// Linux: WebKitGTK lives in a GTK box of the window (works on Wayland and X11).
#[cfg(feature = "webview")]
trait BuildFor {
    fn build_for(self, window: &tao::window::Window) -> wry::Result<wry::WebView>;
}

#[cfg(feature = "webview")]
impl BuildFor for WebViewBuilder<'_> {
    #[cfg(target_os = "linux")]
    fn build_for(self, window: &tao::window::Window) -> wry::Result<wry::WebView> {
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;
        self.build_gtk(window.default_vbox().expect("window box"))
    }

    #[cfg(not(target_os = "linux"))]
    fn build_for(self, window: &tao::window::Window) -> wry::Result<wry::WebView> {
        self.build(window)
    }
}

/// The program that opens the sign-in window: this one, or `RyM` beside it in builds without one.
fn sign_in_program() -> anyhow::Result<std::path::PathBuf> {
    let exe = std::env::current_exe()?;
    if cfg!(feature = "webview") {
        return Ok(exe);
    }
    let rym = exe.with_file_name(format!("RyM{}", std::env::consts::EXE_SUFFIX));
    anyhow::ensure!(rym.is_file(), "signing in needs the RyM program in the same folder ({})", rym.display());
    Ok(rym)
}

/// Opens the sign-in window and waits; returns the cookie header on success.
pub async fn sign_in() -> anyhow::Result<String> {
    let exe = sign_in_program()?;
    let status = tokio::process::Command::new(exe)
        .arg("--login")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await?;
    if !status.success() {
        anyhow::bail!("sign-in window was closed");
    }
    Ok(tokio::fs::read_to_string(crate::cookie_path()).await?)
}
