//! veilq's own window on Windows (ADR-0029): the web UI in WebView2, in a
//! window that belongs to veilq.exe, so the taskbar, Alt+Tab and a pinned
//! entry show veilq and its icon. The server runs on a thread of its own;
//! closing the window ends veilq. Without the WebView2 runtime the caller
//! falls back to a browser's app window.

use std::sync::mpsc;
use std::time::Duration;

use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::window::{Icon, WindowBuilder};
use wry::WebViewBuilder;

use crate::ui::{Options, WindowHooks};

/// Whether the WebView2 runtime is installed (Windows 11 has it, Windows 10
/// with its updates too).
pub fn available() -> bool {
    wry::webview_version().is_ok()
}

enum UserEvent {
    /// The server ended (Quit, uninstall, update).
    Ended,
    /// A second double click: to the front.
    Focus,
}

/// The icon of veilq.exe, for the window's title bar and taskbar button.
fn icon() -> Option<Icon> {
    let png = include_bytes!("../../assets/veilq.png");
    let img = image::load_from_memory(png).ok()?.into_rgba8();
    let (w, h) = img.dimensions();
    Icon::from_rgba(img.into_raw(), w, h).ok()
}

/// Runs the UI server with `options` and shows it in a window until the
/// window is closed or the server ends.
pub fn run(mut options: Options) -> anyhow::Result<()> {
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let (ready_tx, ready_rx) = mpsc::channel::<String>();
    let focus = event_loop.create_proxy();
    let ended = event_loop.create_proxy();
    options.window = Some(WindowHooks {
        ready: ready_tx,
        focus: Box::new(move || {
            let _ = focus.send_event(UserEvent::Focus);
        }),
    });
    let (error_tx, error_rx) = mpsc::channel::<anyhow::Error>();
    std::thread::spawn(move || {
        if let Err(e) = crate::ui::run(options) {
            let _ = error_tx.send(e);
        }
        let _ = ended.send_event(UserEvent::Ended);
    });
    let url = match ready_rx.recv_timeout(Duration::from_secs(120)) {
        Ok(url) => url,
        Err(_) => {
            return Err(error_rx
                .try_recv()
                .unwrap_or_else(|_| anyhow::anyhow!("the UI server did not start")));
        }
    };

    let window = WindowBuilder::new()
        .with_title("veilq")
        .with_window_icon(icon())
        .with_inner_size(LogicalSize::new(1400.0, 900.0))
        .with_min_inner_size(LogicalSize::new(720.0, 480.0))
        .build(&event_loop)?;
    // WebView2 keeps its profile beside the program unless told otherwise:
    // in the Downloads folder, for a veilq.exe started from there.
    let mut context = wry::WebContext::new(Some(super::app_dir().join("webview")));
    let _webview = WebViewBuilder::new_with_web_context(&mut context)
        .with_url(&url)
        // A link that opens a new window (the release notes) goes to the
        // default browser, not into a second window of veilq.
        .with_new_window_req_handler(|target, _| {
            crate::ui::open_browser(&target);
            wry::NewWindowResponse::Deny
        })
        .build(&window)?;

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            }
            | Event::UserEvent(UserEvent::Ended) => {
                let _ = std::fs::remove_file(super::instance_file());
                *control_flow = ControlFlow::Exit;
            }
            Event::UserEvent(UserEvent::Focus) => {
                window.set_minimized(false);
                window.set_focus();
            }
            _ => {}
        }
    })
}
