//! Google sign-in through WebView2, run in its own process (`--login`). Writes the YouTube
//! cookie header to `cookie.txt` and exits.

use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoop};
use tao::window::WindowBuilder;
#[cfg(windows)]
use wry::WebViewBuilderExtWindows;
use wry::{PageLoadEvent, WebContext, WebViewBuilder};

const SIGN_IN: &str = "https://accounts.google.com/ServiceLogin?service=youtube&passive=true\
&continue=https%3A%2F%2Fmusic.youtube.com%2F";
const YTM: &str = "https://music.youtube.com/";

/// WebView2 normally takes part in Google's device-bound sessions: the cookies only live a few
/// minutes and only this window could renew them, so a copied session dies right after sign-in.
/// With the feature off, Google hands out a regular session, like it does for Firefox.
/// (The first three are wry's defaults, which this setting replaces.)
#[cfg(windows)]
const BROWSER_ARGS: &str = "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection,EnableBoundSessionCredentials,DeviceBoundSessions";

/// After the page finishes loading, give its scripts time to set cookies.
const SETTLE: Duration = Duration::from_secs(4);

/// Cookies YouTube Music needs to treat a request as signed in.
fn signed_in(names: &[&str]) -> bool {
    names.contains(&"SAPISID") && names.contains(&"LOGIN_INFO")
}

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
        .build(&window)
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
                let Ok(cookies) = webview.cookies_for_url(YTM) else { return };
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
                let ok = std::fs::write(crate::cookie_path(), &header).is_ok()
                    && crate::stream::write_cookie_file(&header, &crate::ytdlp_cookie_path()).is_ok();
                *flow = ControlFlow::ExitWithCode(if ok { 0 } else { 1 });
            }
            _ => {}
        }
    })
}

/// Opens the sign-in window and waits; returns the cookie header on success.
pub async fn sign_in() -> anyhow::Result<String> {
    let exe = std::env::current_exe()?;
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
