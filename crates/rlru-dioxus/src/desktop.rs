pub(crate) const APP_CSS: &str = include_str!("../assets/styles.css");
#[cfg(feature = "desktop")]
pub(crate) const APP_ID: &str = "org.colonelpanic.rlru.dioxus";
#[cfg(feature = "desktop")]
pub(crate) const APP_ICON_PNG: &[u8] = include_bytes!("../assets/icons/rlru-icon-1024.png");
#[cfg(all(
    feature = "desktop",
    unix,
    not(any(target_os = "ios", target_os = "android"))
))]
const DESKTOP_INSTANCE_SOCKET: &str = "rlru-dioxus.sock";

#[cfg(feature = "desktop")]
fn desktop_head() -> String {
    format!("<style>{APP_CSS}</style>")
}

#[cfg(feature = "desktop")]
fn desktop_data_dir() -> std::path::PathBuf {
    std::env::var_os("XDG_CACHE_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .map(std::path::PathBuf::from)
                .map(|home| home.join(".cache"))
        })
        .unwrap_or_else(std::env::temp_dir)
        .join("rlru-dioxus-webview")
}

#[cfg(all(feature = "desktop", target_os = "linux"))]
fn configure_linux_desktop_environment() {
    if std::env::var("XDG_SESSION_TYPE").unwrap_or_default() == "wayland" {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        std::env::set_var("GDK_BACKEND", "x11");
    }

    glib::set_application_name("rlru");
    if let Err(error) = gtk::init() {
        eprintln!("Failed to initialize GTK before configuring rlru desktop identity: {error}");
        return;
    }

    gdk::set_program_class(APP_ID);
    gtk::Window::set_default_icon_name(APP_ID);
}

#[cfg(not(all(feature = "desktop", target_os = "linux")))]
#[allow(dead_code)]
fn configure_linux_desktop_environment() {}

#[cfg(all(
    feature = "desktop",
    not(any(target_os = "ios", target_os = "android"))
))]
type DesktopWindow = dioxus::desktop::tao::window::Window;

#[cfg(all(
    feature = "desktop",
    not(any(target_os = "ios", target_os = "android"))
))]
type DesktopWindowHandle = std::sync::Arc<DesktopWindow>;

#[cfg(all(
    feature = "desktop",
    not(any(target_os = "ios", target_os = "android"))
))]
type SharedDesktopWindows = std::sync::Arc<std::sync::Mutex<Vec<DesktopWindowHandle>>>;

#[cfg(all(
    feature = "desktop",
    not(any(target_os = "ios", target_os = "android"))
))]
fn shared_desktop_windows() -> SharedDesktopWindows {
    std::sync::Arc::new(std::sync::Mutex::new(Vec::new()))
}

#[cfg(all(
    feature = "desktop",
    unix,
    not(any(target_os = "ios", target_os = "android"))
))]
fn desktop_instance_socket_path() -> std::path::PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join(DESKTOP_INSTANCE_SOCKET)
}

#[cfg(all(
    feature = "desktop",
    unix,
    not(any(target_os = "ios", target_os = "android"))
))]
fn notify_existing_desktop_instance() -> bool {
    use std::io::Write;
    use std::os::unix::net::UnixStream;

    UnixStream::connect(desktop_instance_socket_path())
        .and_then(|mut stream| stream.write_all(b"show\n"))
        .is_ok()
}

#[cfg(all(
    feature = "desktop",
    not(any(target_os = "ios", target_os = "android"))
))]
fn restore_desktop_window(window: &DesktopWindow) {
    window.set_visible(true);
    window.set_minimized(false);
    window.set_focus();
}

#[cfg(all(
    feature = "desktop",
    not(any(target_os = "ios", target_os = "android"))
))]
pub(crate) fn restore_desktop_window_handle(window: DesktopWindowHandle) {
    restore_desktop_window(&window);

    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(75));
        restore_desktop_window(&window);

        std::thread::sleep(std::time::Duration::from_millis(150));
        window.set_focus();
    });
}

#[cfg(all(
    feature = "desktop",
    unix,
    not(any(target_os = "ios", target_os = "android"))
))]
fn restore_desktop_windows(windows: &SharedDesktopWindows) {
    let Ok(windows) = windows.lock() else {
        return;
    };

    for window in windows.iter() {
        restore_desktop_window_handle(window.clone());
    }
}

#[cfg(all(
    feature = "desktop",
    unix,
    not(any(target_os = "ios", target_os = "android"))
))]
fn run_desktop_instance_listener(
    listener: std::os::unix::net::UnixListener,
    windows: SharedDesktopWindows,
) {
    use std::io::Read;

    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else {
                continue;
            };

            let mut message = [0_u8; 16];
            if stream.read(&mut message).is_ok() {
                restore_desktop_windows(&windows);
            }
        }
    });
}

#[cfg(all(
    feature = "desktop",
    unix,
    not(any(target_os = "ios", target_os = "android"))
))]
fn start_desktop_instance_listener(windows: SharedDesktopWindows) -> bool {
    use std::os::unix::net::UnixListener;

    let socket_path = desktop_instance_socket_path();
    if let Some(parent) = socket_path.parent() {
        if let Err(error) = std::fs::create_dir_all(parent) {
            eprintln!("Failed to create rlru instance socket directory: {error}");
            return true;
        }
    }

    match UnixListener::bind(&socket_path) {
        Ok(listener) => {
            run_desktop_instance_listener(listener, windows);
            true
        }
        Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {
            if notify_existing_desktop_instance() {
                return false;
            }

            if let Err(remove_error) = std::fs::remove_file(&socket_path) {
                eprintln!("Failed to remove stale rlru instance socket: {remove_error}");
                return true;
            }

            match UnixListener::bind(&socket_path) {
                Ok(listener) => {
                    run_desktop_instance_listener(listener, windows);
                    true
                }
                Err(error) => {
                    eprintln!("Failed to bind rlru instance socket after stale cleanup: {error}");
                    true
                }
            }
        }
        Err(error) => {
            eprintln!("Failed to bind rlru instance socket: {error}");
            true
        }
    }
}

#[cfg(any(
    not(feature = "desktop"),
    not(unix),
    target_os = "ios",
    target_os = "android"
))]
#[allow(dead_code)]
pub(crate) fn cleanup_desktop_instance_socket() {}

#[cfg(all(
    feature = "desktop",
    unix,
    not(any(target_os = "ios", target_os = "android"))
))]
pub(crate) fn cleanup_desktop_instance_socket() {
    let _ = std::fs::remove_file(desktop_instance_socket_path());
}

#[cfg(feature = "desktop")]
pub(crate) fn launch_app() {
    use dioxus::desktop::{icon_from_memory, Config, WindowBuilder, WindowCloseBehaviour};

    configure_linux_desktop_environment();

    let windows = shared_desktop_windows();
    #[cfg(all(unix, not(any(target_os = "ios", target_os = "android"))))]
    {
        if notify_existing_desktop_instance() {
            return;
        }

        if !start_desktop_instance_listener(windows.clone()) {
            return;
        }
    }

    let mut config = Config::new()
        .with_custom_head(desktop_head())
        .with_data_directory(desktop_data_dir())
        .with_background_color((243, 246, 244, 255))
        .with_close_behaviour(WindowCloseBehaviour::WindowCloses)
        .with_menu(None)
        .with_on_window(move |window, _| {
            if let Ok(mut windows) = windows.lock() {
                windows.push(window);
            }
        })
        .with_window(WindowBuilder::new().with_title("rlru").with_visible(true));

    match icon_from_memory::<dioxus::desktop::tao::window::Icon>(APP_ICON_PNG) {
        Ok(icon) => config = config.with_icon(icon),
        Err(error) => eprintln!("Failed to load rlru window icon: {error}"),
    }

    #[cfg(all(unix, not(any(target_os = "ios", target_os = "android"))))]
    {
        use dioxus::desktop::tao::event_loop::EventLoopBuilder;
        use dioxus::desktop::tao::platform::unix::EventLoopBuilderExtUnix;

        let mut event_loop = EventLoopBuilder::with_user_event();
        event_loop.with_app_id(APP_ID);
        config = config.with_event_loop(event_loop.build());
    }

    dioxus::LaunchBuilder::desktop()
        .with_cfg(config)
        .launch(crate::App);
}
