//! Desktop side: a tray icon by the clock and a native window that shows the
//! UI. The window is created when asked for and destroyed when closed, so the
//! WebView2 processes only run while it is open.
//!
//! The page and its API are served to the window through a private `spt://`
//! protocol, not over the network: nothing else on the PC can reach the API,
//! which matters because the app runs as administrator.

use std::path::PathBuf;

use anyhow::{Context, Result};
use axum::Router;
use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tao::window::{Window, WindowBuilder};
use tower::ServiceExt;
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use wry::{WebContext, WebView, WebViewBuilder};

/// The page's address inside the window (`spt://localhost/` as WebView2 sees it).
pub const PAGE_URL: &str = "http://spt.localhost/";

#[derive(Debug)]
pub enum UiEvent {
    ShowWindow,
    Quit,
    Tray(TrayIconEvent),
    Menu(MenuEvent),
}

/// Lets other threads (the instance port, the API) drive the event loop.
#[derive(Clone)]
pub struct Handle(EventLoopProxy<UiEvent>);

impl Handle {
    pub fn show(&self) {
        let _ = self.0.send_event(UiEvent::ShowWindow);
    }
    pub fn quit(&self) {
        let _ = self.0.send_event(UiEvent::Quit);
    }
}

pub struct Desktop {
    event_loop: tao::event_loop::EventLoop<UiEvent>,
}

impl Desktop {
    /// Must be called on the main thread.
    pub fn new() -> Self {
        Self {
            event_loop: EventLoopBuilder::<UiEvent>::with_user_event().build(),
        }
    }

    pub fn handle(&self) -> Handle {
        Handle(self.event_loop.create_proxy())
    }

    /// Runs the tray and window until Quit, then exits the process.
    pub fn run(
        self,
        router: Router,
        runtime: tokio::runtime::Handle,
        data_dir: PathBuf,
        show_at_start: bool,
        dark: impl Fn() -> bool + 'static,
    ) -> Result<()> {
        let proxy = self.event_loop.create_proxy();
        TrayIconEvent::set_event_handler(Some(move |e| {
            let _ = proxy.send_event(UiEvent::Tray(e));
        }));
        let proxy = self.event_loop.create_proxy();
        MenuEvent::set_event_handler(Some(move |e| {
            let _ = proxy.send_event(UiEvent::Menu(e));
        }));

        let open = MenuItem::new("Open Simple PC Tools", true, None);
        let quit = MenuItem::new("Quit", true, None);
        let menu = Menu::with_items(&[&open, &PredefinedMenuItem::separator(), &quit])
            .context("building tray menu")?;
        let tray = TrayIconBuilder::new()
            .with_tooltip("Simple PC Tools")
            .with_icon(tray_icon_image()?)
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .build()
            .context("creating tray icon")?;

        let mut web_context = WebContext::new(Some(data_dir.join("webview")));
        let mut open_window: Option<(Window, WebView)> = None;
        if show_at_start {
            let _ = self
                .event_loop
                .create_proxy()
                .send_event(UiEvent::ShowWindow);
        }

        self.event_loop.run(move |event, target, control_flow| {
            *control_flow = ControlFlow::Wait;
            let _keep = &tray;
            let mut show = false;
            match event {
                Event::UserEvent(UiEvent::ShowWindow) => show = true,
                Event::UserEvent(UiEvent::Quit) => {
                    tracing::info!("quitting; apps it started keep running");
                    open_window = None;
                    *control_flow = ControlFlow::Exit;
                }
                Event::UserEvent(UiEvent::Tray(TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                })) => show = true,
                Event::UserEvent(UiEvent::Menu(e)) => {
                    if e.id == *open.id() {
                        show = true;
                    } else if e.id == *quit.id() {
                        tracing::info!("quit from tray menu; apps it started keep running");
                        open_window = None;
                        *control_flow = ControlFlow::Exit;
                    }
                }
                Event::WindowEvent {
                    event: WindowEvent::CloseRequested,
                    ..
                } => open_window = None,
                _ => {}
            }
            if !show {
                return;
            }
            if let Some((window, _)) = &open_window {
                window.set_visible(true);
                window.set_minimized(false);
                window.set_focus();
                return;
            }
            match build_window(
                target,
                &mut web_context,
                router.clone(),
                runtime.clone(),
                dark(),
            ) {
                Ok(w) => open_window = Some(w),
                Err(e) => tracing::error!("could not open the window: {e:#}"),
            }
        })
    }
}

fn build_window(
    target: &tao::event_loop::EventLoopWindowTarget<UiEvent>,
    web_context: &mut WebContext,
    router: Router,
    runtime: tokio::runtime::Handle,
    dark: bool,
) -> Result<(Window, WebView)> {
    use tao::platform::windows::IconExtWindows;
    // Matches --bg in ui/index.html so the window never flashes while loading.
    let bg = if dark {
        (0x20, 0x20, 0x20, 255)
    } else {
        (0xff, 0xff, 0xff, 255)
    };
    let window = WindowBuilder::new()
        .with_title("Simple PC Tools")
        .with_inner_size(LogicalSize::new(1080.0, 760.0))
        .with_min_inner_size(LogicalSize::new(560.0, 480.0))
        .with_window_icon(tao::window::Icon::from_resource(APP_ICON, None).ok())
        .with_theme(Some(if dark {
            tao::window::Theme::Dark
        } else {
            tao::window::Theme::Light
        }))
        .with_background_color(bg)
        .build(target)
        .context("creating window")?;
    let webview = WebViewBuilder::new_with_web_context(web_context)
        .with_asynchronous_custom_protocol("spt".into(), move |_id, request, responder| {
            let router = router.clone();
            runtime.spawn(async move {
                tracing::debug!("window: {} {}", request.method(), request.uri());
                let (parts, body) = request.into_parts();
                let request = axum::http::Request::from_parts(parts, axum::body::Body::from(body));
                let response = match router.oneshot(request).await {
                    Ok(r) => r,
                    Err(never) => match never {},
                };
                let (parts, body) = response.into_parts();
                let bytes = axum::body::to_bytes(body, usize::MAX)
                    .await
                    .unwrap_or_default();
                responder.respond(axum::http::Response::from_parts(parts, bytes.to_vec()));
            });
        })
        .with_url(PAGE_URL)
        .with_background_color(bg)
        // Links to other sites (GitHub, docs) open in the default browser.
        .with_navigation_handler(|url| {
            let inside = url.starts_with(PAGE_URL);
            if !inside && url.starts_with("https://") {
                open_in_browser(&url);
            }
            inside
        })
        .with_new_window_req_handler(|url, _features| {
            if url.starts_with("https://") {
                open_in_browser(&url);
            }
            wry::NewWindowResponse::Deny
        })
        .with_devtools(cfg!(debug_assertions))
        .build(&window)
        .context("creating WebView2 view")?;
    window.set_focus();
    Ok((window, webview))
}

pub fn open_in_browser(url: &str) {
    // `start` treats its first quoted argument as a title, hence the "".
    let mut cmd = std::process::Command::new("cmd");
    cmd.args(["/C", "start", "", url]);
    crate::tools::apps::action::no_window(&mut cmd);
    let _ = cmd.spawn();
}

/// Resource id of assets/app.ico, embedded by build.rs.
const APP_ICON: u16 = 1;

/// The app icon at the tray's size for this display's DPI (16 px at 100 %), so
/// Windows picks the simplified small frame instead of shrinking the big one.
fn tray_icon_image() -> Result<tray_icon::Icon> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSMICON};
    // SAFETY: no arguments to validate.
    let px = unsafe { GetSystemMetrics(SM_CXSMICON) }.max(16) as u32;
    Ok(tray_icon::Icon::from_resource(APP_ICON, Some((px, px)))?)
}

/// Whether Windows apps are set to dark mode.
pub fn system_uses_dark_theme() -> bool {
    let mut cmd = std::process::Command::new("reg");
    cmd.args([
        "query",
        r"HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
        "/v",
        "AppsUseLightTheme",
    ]);
    crate::tools::apps::action::no_window(&mut cmd);
    matches!(cmd.output(), Ok(o) if String::from_utf8_lossy(&o.stdout).contains("0x0"))
}
