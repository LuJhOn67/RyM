//! Google sign-in through WebView2, run in its own process (`--login` / `--refresh`).
//!
//! Google rotates part of the session cookies every few hours; a browser does that in the
//! background. `--refresh` loads YouTube Music in a hidden window with the saved WebView
//! profile, so the page rotates them, then saves the fresh cookies.

use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoop};
use tao::window::WindowBuilder;
use wry::{PageLoadEvent, WebContext, WebViewBuilder};

const SIGN_IN: &str = "https://accounts.google.com/ServiceLogin?service=youtube&passive=true\
&continue=https%3A%2F%2Fmusic.youtube.com%2F";
const YTM: &str = "https://music.youtube.com/";

/// After the page finishes loading, give its scripts time to rotate cookies.
const SETTLE: Duration = Duration::from_secs(4);
const REFRESH_TIMEOUT: Duration = Duration::from_secs(30);

/// Cookies YouTube Music needs to treat a request as signed in.
fn signed_in(names: &[&str]) -> bool {
    names.contains(&"SAPISID") && names.contains(&"LOGIN_INFO")
}

/// `silent`: hidden window that only refreshes an existing session.
pub fn run(silent: bool) -> ! {
    let event_loop = EventLoop::new();
    let window = WindowBuilder::new()
        .with_title("Sign in to YouTube Music")
        .with_inner_size(tao::dpi::LogicalSize::new(520.0, 720.0))
        .with_visible(!silent)
        .build(&event_loop)
        .expect("login window");
    let mut ctx = WebContext::new(Some(crate::data_dir().join("webview")));

    // Time the last music.youtube.com page finished loading.
    let loaded: Rc<Cell<Option<Instant>>> = Rc::new(Cell::new(None));
    let on_load = loaded.clone();
    let webview = WebViewBuilder::new_with_web_context(&mut ctx)
        .with_url(if silent { YTM } else { SIGN_IN })
        .with_on_page_load_handler(move |ev, url| {
            if matches!(ev, PageLoadEvent::Finished) && url.starts_with("https://music.youtube.com") {
                on_load.set(Some(Instant::now()));
            }
        })
        .build(&window)
        .expect("webview");
    let started = Instant::now();

    event_loop.run(move |event, _, flow| {
        *flow = ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(500));
        match event {
            // Exit through the event loop (not process::exit) so WebView2 shuts down cleanly
            // and saves the signed-in profile to disk.
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => *flow = ControlFlow::ExitWithCode(1),
            Event::NewEvents(_) => {
                if silent && started.elapsed() > REFRESH_TIMEOUT {
                    *flow = ControlFlow::ExitWithCode(2);
                    return;
                }
                if !loaded.get().is_some_and(|t| t.elapsed() >= SETTLE) {
                    return;
                }
                let Ok(cookies) = webview.cookies_for_url(YTM) else { return };
                let names: Vec<&str> = cookies.iter().map(|c| c.name()).collect();
                if std::env::var_os("YTM_DEBUG").is_some() {
                    // Names only: values are credentials.
                    let url = webview.url().unwrap_or_default();
                    let _ = std::fs::write(crate::data_dir().join("debug-cookies.txt"), format!("{url}\n{names:?}"));
                }
                if !signed_in(&names) {
                    if silent {
                        // The saved profile is signed out; a visible sign-in is needed.
                        *flow = ControlFlow::ExitWithCode(3);
                    }
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

async fn spawn_self(flag: &str) -> anyhow::Result<String> {
    let exe = std::env::current_exe()?;
    let status = tokio::process::Command::new(exe)
        .arg(flag)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await?;
    match status.code() {
        Some(0) => Ok(tokio::fs::read_to_string(crate::cookie_path()).await?),
        Some(3) => anyhow::bail!("session expired, sign in again"),
        _ if flag == "--login" => anyhow::bail!("sign-in window was closed"),
        _ => anyhow::bail!("couldn't refresh the session"),
    }
}

/// Opens the sign-in window and waits; returns the cookie header on success.
pub async fn sign_in() -> anyhow::Result<String> {
    spawn_self("--login").await
}

/// Refreshes the saved session in a hidden window; returns the new cookie header.
pub async fn refresh() -> anyhow::Result<String> {
    spawn_self("--refresh").await
}
