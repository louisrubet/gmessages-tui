//! Does Google accept signing in from a webview? Opens the Google sign-in in a wry window,
//! and says whether it ends on Google Messages (accepted) or on Google's rejection page.
//!
//!   cargo run --example wry_signin --features wry-signin
//!
//! Nothing is persisted by the private webview. Page addresses and the SOCS cookie
//! (when present after successful sign-in) are printed to stdout.

use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::window::WindowBuilder;
use wry::{PageLoadEvent, WebViewBuilder};

const SIGN_IN: &str =
    "https://accounts.google.com/AccountChooser?continue=https://messages.google.com/web/config";

enum Verdict {
    Accepted,
    Refused,
}

fn print_socs(webview: &wry::WebView) {
    // Inspect this private webview's entire cookie store: SOCS may belong to a
    // different Google host and therefore be excluded by cookies_for_url().
    match webview.cookies() {
        Ok(cookies) => {
            eprintln!("Cookies in the private webview: {}", cookies.len());
            let mut found = false;
            for cookie in cookies.iter().filter(|cookie| cookie.name() == "SOCS") {
                found = true;
                println!("SOCS={}", cookie.value());
            }
            if !found {
                eprintln!("No SOCS cookie in the private webview at this point.");
                // Metadata only: never dump authentication cookie values.
                for cookie in &cookies {
                    eprintln!(
                        "cookie name={} domain={} path={}",
                        cookie.name(),
                        cookie.domain().unwrap_or("<unspecified>"),
                        cookie.path().unwrap_or("<unspecified>"),
                    );
                }
            }
        }
        Err(error) => eprintln!("Cannot read cookies: {error}"),
    }
}

fn main() -> wry::Result<()> {
    // WebKitGTK's DMA-BUF renderer breaks on some Wayland setups ("Error 71 (Protocol error)
    // dispatching to Wayland display").
    #[cfg(target_os = "linux")]
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        // SAFETY: no other thread yet.
        unsafe { std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1") };
    }
    let event_loop = EventLoopBuilder::<Verdict>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    let window = WindowBuilder::new()
        .with_title("Google sign-in test")
        .build(&event_loop)
        .expect("cannot open a window");

    let builder = WebViewBuilder::new()
        .with_incognito(true)
        .with_url(SIGN_IN)
        .with_on_page_load_handler(move |event, url| {
            if !matches!(event, PageLoadEvent::Finished) {
                return;
            }
            // The address without its query: it may carry tokens.
            let page = url.split(['?', '#']).next().unwrap_or_default();
            println!("loaded {page}");
            if page.starts_with("https://messages.google.com/web/config") {
                let _ = proxy.send_event(Verdict::Accepted);
            } else if page.contains("/rejected") {
                let _ = proxy.send_event(Verdict::Refused);
            }
        });

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    let webview = builder.build(&window)?;
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let webview = {
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;
        builder.build_gtk(window.default_vbox().expect("a GTK window"))?
    };

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::UserEvent(Verdict::Accepted) => {
                println!("Google ACCEPTED the sign-in from the webview.");
                print_socs(&webview);
                *control_flow = ControlFlow::Exit;
            }
            Event::UserEvent(Verdict::Refused) => {
                println!("Google REFUSED the sign-in from the webview.");
                *control_flow = ControlFlow::Exit;
            }
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                println!("Window closed: no verdict.");
                *control_flow = ControlFlow::Exit;
            }
            _ => {}
        }
    });
}
