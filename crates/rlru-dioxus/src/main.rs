use dioxus::prelude::*;

#[cfg(not(target_arch = "wasm32"))]
use rlru::app::{
    add_account, backfill_upload_destinations, dedupe_upload_requests, failed_upload,
    format_backfill_message, is_same_upload, is_same_upload_request, load_history, load_summary,
    now_label, remove_account, save_auto_upload, save_overview_config, short_match_id,
    upload_failure_reason, upload_history_replay, upsert_failed_upload, AccountFormData,
    AppSummary, HistoryRow, OverviewConfigFormData, ReplayUploadRequest, SyncRunState,
};
#[cfg(all(
    not(target_arch = "wasm32"),
    feature = "desktop",
    not(any(target_os = "ios", target_os = "android"))
))]
use rlru::app::{
    auto_upload_label, format_failed_upload_retry_label, tray_sync_label, tray_tooltip,
};

const APP_CSS: &str = include_str!("../assets/styles.css");
#[cfg(feature = "desktop")]
const APP_ICON_PNG: &[u8] = include_bytes!("../assets/icons/rlru-icon-1024.png");
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
fn restore_desktop_window_handle(window: DesktopWindowHandle) {
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
fn cleanup_desktop_instance_socket() {}

#[cfg(all(
    feature = "desktop",
    unix,
    not(any(target_os = "ios", target_os = "android"))
))]
fn cleanup_desktop_instance_socket() {
    let _ = std::fs::remove_file(desktop_instance_socket_path());
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ActiveView {
    Overview,
    History,
    Accounts,
    UploadDestinations,
}

impl ActiveView {
    const ALL: [Self; 4] = [
        Self::Overview,
        Self::History,
        Self::Accounts,
        Self::UploadDestinations,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::History => "History",
            Self::Accounts => "Accounts",
            Self::UploadDestinations => "Upload Destinations",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::Overview => "Local auth, typed config, replay upload destinations",
            Self::History => "Current RL API matches and upload destination state",
            Self::Accounts => "Configured Rocket League account credentials",
            Self::UploadDestinations => "Replay destinations, upload mode, and activity",
        }
    }
}

fn main() {
    launch_app();
}

#[cfg(feature = "desktop")]
fn launch_app() {
    use dioxus::desktop::{icon_from_memory, Config, WindowBuilder, WindowCloseBehaviour};

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

    dioxus::LaunchBuilder::desktop()
        .with_cfg(config)
        .launch(App);
}

#[cfg(not(feature = "desktop"))]
fn launch_app() {
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    let mut summary = use_signal(load_summary);
    let mut active_view = use_signal(|| ActiveView::Overview);
    let mut history_requested = use_signal(|| false);
    let mut history_refresh_tick = use_signal(|| 0_u64);
    let history = use_resource(move || async move {
        if history_requested() {
            let _ = history_refresh_tick();
            load_history().await
        } else {
            std::future::pending::<Result<Vec<HistoryRow>, String>>().await
        }
    });
    let mut action_message = use_signal(String::new);
    let mut history_message = use_signal(String::new);
    let mut backfill_running = use_signal(|| false);
    let mut uploading_replay = use_signal(|| None::<ReplayUploadRequest>);
    let mut sync_run = use_signal(SyncRunState::default);
    let mut failed_uploads = use_signal(Vec::<ReplayUploadRequest>::new);
    let active = active_view();
    let current_summary = summary();
    let message = action_message();
    let history_has_been_requested = history_requested();
    let current_history = if history_has_been_requested || active == ActiveView::History {
        history.cloned()
    } else {
        Some(Ok(Vec::new()))
    };
    let history_status = history_message();
    let is_backfill_running = backfill_running();
    let current_uploading_replay = uploading_replay();
    let current_sync_run = sync_run();
    let current_failed_uploads = failed_uploads();

    use_effect(move || {
        if active_view() == ActiveView::History && !history_requested() {
            history_requested.set(true);
        }
    });

    rsx! {
        document::Title { "rlru" }
        document::Meta {
            name: "viewport",
            content: "width=device-width, initial-scale=1, viewport-fit=cover",
        }
        document::Style { "{APP_CSS}" }
        DesktopTrayBridge {
            summary: current_summary.clone(),
            history: current_history.clone(),
            sync_run: current_sync_run.clone(),
            failed_uploads: current_failed_uploads.clone(),
            onsync: move |_| {
                backfill_running.set(true);
                let started_at = now_label();
                sync_run.set(SyncRunState {
                    running: true,
                    last_started_at: Some(started_at),
                    last_completed_at: sync_run().last_completed_at.clone(),
                    last_summary: sync_run().last_summary.clone(),
                    last_error: None,
                });
                history_message.set("Syncing upload destinations from current RL API history".to_string());
                spawn(async move {
                    match backfill_upload_destinations().await {
                        Ok(run_summary) => {
                            failed_uploads.set(dedupe_upload_requests(run_summary.failed_uploads.clone()));
                            sync_run.set(SyncRunState {
                                running: false,
                                last_started_at: sync_run().last_started_at.clone(),
                                last_completed_at: Some(now_label()),
                                last_summary: Some(run_summary.clone()),
                                last_error: None,
                            });
                            history_message.set(format_backfill_message(
                                format!(
                                    "Sync complete: {} uploaded, {} duplicates, {} cached, {} failed",
                                    run_summary.uploaded,
                                    run_summary.duplicates,
                                    run_summary.cached,
                                    run_summary.failed
                                ),
                                &run_summary.failed_uploads,
                            ));
                            history_refresh_tick.set(history_refresh_tick().wrapping_add(1));
                        }
                        Err(error) => {
                            sync_run.set(SyncRunState {
                                running: false,
                                last_started_at: sync_run().last_started_at.clone(),
                                last_completed_at: Some(now_label()),
                                last_summary: sync_run().last_summary.clone(),
                                last_error: Some(error.clone()),
                            });
                            history_message.set(error);
                        }
                    }
                    backfill_running.set(false);
                });
            },
            onrefreshhistory: move |_| {
                history_requested.set(true);
                history_refresh_tick.set(history_refresh_tick().wrapping_add(1));
                history_message.set(String::new());
            },
            onretry: move |request: ReplayUploadRequest| {
                uploading_replay.set(Some(request.clone()));
                let started_at = now_label();
                sync_run.set(SyncRunState {
                    running: true,
                    last_started_at: Some(started_at),
                    last_completed_at: sync_run().last_completed_at.clone(),
                    last_summary: sync_run().last_summary.clone(),
                    last_error: None,
                });
                history_message.set(format!(
                    "Retrying {} to {}",
                    short_match_id(&request.match_id),
                    request.target_name
                ));
                spawn(async move {
                    match upload_history_replay(request.clone()).await {
                        Ok(run_summary) => {
                            let mut failures = failed_uploads();
                            failures.retain(|failure| !is_same_upload_request(failure, &request));
                            for failure in &run_summary.failed_uploads {
                                upsert_failed_upload(&mut failures, failure.clone());
                            }
                            failed_uploads.set(failures);
                            sync_run.set(SyncRunState {
                                running: false,
                                last_started_at: sync_run().last_started_at.clone(),
                                last_completed_at: Some(now_label()),
                                last_summary: Some(run_summary.clone()),
                                last_error: None,
                            });
                            history_message.set(format_backfill_message(
                                format!(
                                    "Retry to {} complete: {} uploaded, {} duplicates, {} cached, {} failed",
                                    request.target_name,
                                    run_summary.uploaded,
                                    run_summary.duplicates,
                                    run_summary.cached,
                                    run_summary.failed
                                ),
                                &run_summary.failed_uploads,
                            ));
                            history_refresh_tick.set(history_refresh_tick().wrapping_add(1));
                        }
                        Err(error) => {
                            sync_run.set(SyncRunState {
                                running: false,
                                last_started_at: sync_run().last_started_at.clone(),
                                last_completed_at: Some(now_label()),
                                last_summary: sync_run().last_summary.clone(),
                                last_error: Some(error.clone()),
                            });
                            history_message.set(error);
                        }
                    }
                    uploading_replay.set(None);
                });
            },
        }
        main {
            class: "shell",
            Sidebar {
                active,
                onselect: move |view| active_view.set(view),
            }
            section {
                class: "workspace",
                header {
                    class: "topbar",
                    div {
                        h1 { "{active.label()}" }
                        p { "{active.description()}" }
                    }
                    button {
                        class: "primary-button",
                        onclick: move |_| {
                            summary.set(load_summary());
                            action_message.set(String::new());
                        },
                        "Refresh"
                    }
                }
                if !message.is_empty() {
                    div { class: "notice", "{message}" }
                }
                match active {
                    ActiveView::Overview => rsx! {
                        OverviewView {
                            summary: current_summary,
                            onsave: move |input| {
                                match save_overview_config(input) {
                                    Ok(updated) => {
                                        summary.set(updated);
                                        action_message.set("Configuration updated".to_string());
                                    }
                                    Err(error) => action_message.set(error),
                                }
                            },
                        }
                    },
                    ActiveView::History => rsx! {
                        HistoryView {
                            history: current_history,
                            message: history_status,
                            backfill_running: is_backfill_running,
                            uploading: current_uploading_replay,
                            failed_uploads: current_failed_uploads,
                            onrefresh: move |_| {
                                history_requested.set(true);
                                history_refresh_tick.set(history_refresh_tick().wrapping_add(1));
                                history_message.set(String::new());
                            },
                            onbackfill: move |_| {
                                backfill_running.set(true);
                                let started_at = now_label();
                                sync_run.set(SyncRunState {
                                    running: true,
                                    last_started_at: Some(started_at),
                                    last_completed_at: sync_run().last_completed_at.clone(),
                                    last_summary: sync_run().last_summary.clone(),
                                    last_error: None,
                                });
                                history_message.set("Backfilling upload destinations from current RL API history".to_string());
                                spawn(async move {
                                    match backfill_upload_destinations().await {
                                        Ok(run_summary) => {
                                            failed_uploads.set(dedupe_upload_requests(run_summary.failed_uploads.clone()));
                                            sync_run.set(SyncRunState {
                                                running: false,
                                                last_started_at: sync_run().last_started_at.clone(),
                                                last_completed_at: Some(now_label()),
                                                last_summary: Some(run_summary.clone()),
                                                last_error: None,
                                            });
                                            history_message.set(format_backfill_message(
                                                format!(
                                                "Backfill complete: {} uploaded, {} duplicates, {} cached, {} failed",
                                                run_summary.uploaded,
                                                run_summary.duplicates,
                                                run_summary.cached,
                                                run_summary.failed
                                                ),
                                                &run_summary.failed_uploads,
                                            ));
                                            history_refresh_tick.set(history_refresh_tick().wrapping_add(1));
                                        }
                                        Err(error) => {
                                            sync_run.set(SyncRunState {
                                                running: false,
                                                last_started_at: sync_run().last_started_at.clone(),
                                                last_completed_at: Some(now_label()),
                                                last_summary: sync_run().last_summary.clone(),
                                                last_error: Some(error.clone()),
                                            });
                                            history_message.set(error);
                                        }
                                    }
                                    backfill_running.set(false);
                                });
                            },
                            onupload: move |request: ReplayUploadRequest| {
                                uploading_replay.set(Some(request.clone()));
                                let started_at = now_label();
                                sync_run.set(SyncRunState {
                                    running: true,
                                    last_started_at: Some(started_at),
                                    last_completed_at: sync_run().last_completed_at.clone(),
                                    last_summary: sync_run().last_summary.clone(),
                                    last_error: None,
                                });
                                history_message.set(format!(
                                    "Uploading {} to {}",
                                    short_match_id(&request.match_id),
                                    request.target_name
                                ));
                                spawn(async move {
                                    match upload_history_replay(request.clone()).await {
                                        Ok(run_summary) => {
                                            let mut failures = failed_uploads();
                                            failures.retain(|failure| !is_same_upload_request(failure, &request));
                                            for failure in &run_summary.failed_uploads {
                                                upsert_failed_upload(&mut failures, failure.clone());
                                            }
                                            failed_uploads.set(failures);
                                            sync_run.set(SyncRunState {
                                                running: false,
                                                last_started_at: sync_run().last_started_at.clone(),
                                                last_completed_at: Some(now_label()),
                                                last_summary: Some(run_summary.clone()),
                                                last_error: None,
                                            });
                                            history_message.set(format_backfill_message(
                                                format!(
                                                "Upload to {} complete: {} uploaded, {} duplicates, {} cached, {} failed",
                                                request.target_name,
                                                run_summary.uploaded,
                                                run_summary.duplicates,
                                                run_summary.cached,
                                                run_summary.failed
                                                ),
                                                &run_summary.failed_uploads,
                                            ));
                                            history_refresh_tick.set(history_refresh_tick().wrapping_add(1));
                                        }
                                        Err(error) => {
                                            sync_run.set(SyncRunState {
                                                running: false,
                                                last_started_at: sync_run().last_started_at.clone(),
                                                last_completed_at: Some(now_label()),
                                                last_summary: sync_run().last_summary.clone(),
                                                last_error: Some(error.clone()),
                                            });
                                            history_message.set(error);
                                        }
                                    }
                                    uploading_replay.set(None);
                                });
                            },
                        }
                    },
                    ActiveView::Accounts => rsx! {
                        AccountsView {
                            summary: current_summary,
                            onadd: move |input| {
                                match add_account(input) {
                                    Ok(updated) => {
                                        summary.set(updated);
                                        action_message.set("Account added to config".to_string());
                                    }
                                    Err(error) => action_message.set(error),
                                }
                            },
                            onremove: move |account_id| {
                                match remove_account(account_id) {
                                    Ok(updated) => {
                                        summary.set(updated);
                                        action_message.set("Account removed from config".to_string());
                                    }
                                    Err(error) => action_message.set(error),
                                }
                            },
                        }
                    },
                    ActiveView::UploadDestinations => rsx! {
                        UploadDestinationsView {
                            summary: current_summary,
                            onautoupload: move |enabled| {
                                match save_auto_upload(enabled) {
                                    Ok(updated) => {
                                        summary.set(updated);
                                        action_message.set(if enabled {
                                            "Auto upload enabled in config".to_string()
                                        } else {
                                            "Auto upload disabled in config".to_string()
                                        });
                                    }
                                    Err(error) => action_message.set(error),
                                }
                            },
                        }
                    },
                }
            }
        }
    }
}

#[component]
fn Sidebar(active: ActiveView, onselect: EventHandler<ActiveView>) -> Element {
    rsx! {
        aside {
            class: "sidebar",
            strong { "rlru" }
            nav {
                for view in ActiveView::ALL {
                    NavButton {
                        view,
                        selected: active == view,
                        onclick: move |_| onselect.call(view),
                    }
                }
            }
        }
    }
}

#[component]
fn NavButton(view: ActiveView, selected: bool, onclick: EventHandler<MouseEvent>) -> Element {
    let class = if selected {
        "nav-button selected"
    } else {
        "nav-button"
    };

    rsx! {
        button {
            class: "{class}",
            onclick: move |event| onclick.call(event),
            "{view.label()}"
        }
    }
}

#[component]
fn OverviewView(summary: AppSummary, onsave: EventHandler<OverviewConfigFormData>) -> Element {
    let auto_upload_value = if summary.auto_upload {
        "Enabled"
    } else {
        "Disabled"
    }
    .to_string();
    let mut interval_minutes = use_signal(|| summary.auto_upload_interval_minutes.to_string());
    let mut jitter_minutes = use_signal(|| summary.auto_upload_jitter_minutes.to_string());
    let mut upload_on_launch = use_signal(|| summary.upload_on_launch);
    let mut no_upload_while_connected = use_signal(|| summary.no_upload_while_connected);

    rsx! {
        div { class: "summary-grid",
            Metric { label: "Accounts", value: summary.account_count().to_string() }
            Metric { label: "Upload Destinations", value: summary.upload_destination_count().to_string() }
            Metric { label: "Auto Upload", value: auto_upload_value }
        }
        section { class: "panel",
            div { class: "panel-header",
                h2 { "Configuration" }
                span { "{summary.interval}" }
            }
            dl { class: "details",
                dt { "Path" }
                dd { "{summary.config_path}" }
                dt { "State" }
                dd { "{summary.status}" }
                dt { "Account" }
                dd { "{summary.selected_account.clone().unwrap_or_else(|| \"All configured accounts\".to_string())}" }
                dt { "Upload Destination" }
                dd { "{summary.selected_upload_destination.clone().unwrap_or_else(|| \"All configured destinations\".to_string())}" }
            }
            div { class: "config-form",
                label {
                    span { "Sync interval" }
                    div { class: "input-with-unit",
                        input {
                            r#type: "number",
                            min: "1",
                            value: "{interval_minutes}",
                            oninput: move |event| interval_minutes.set(event.value()),
                        }
                        small { "minutes" }
                    }
                }
                label {
                    span { "Jitter max" }
                    div { class: "input-with-unit",
                        input {
                            r#type: "number",
                            min: "0",
                            value: "{jitter_minutes}",
                            oninput: move |event| jitter_minutes.set(event.value()),
                        }
                        small { "minutes" }
                    }
                }
                label { class: "checkbox-field",
                    input {
                        r#type: "checkbox",
                        checked: upload_on_launch(),
                        oninput: move |event| upload_on_launch.set(event.checked()),
                    }
                    span { "Run sync at launch" }
                }
                label { class: "checkbox-field",
                    input {
                        r#type: "checkbox",
                        checked: no_upload_while_connected(),
                        oninput: move |event| no_upload_while_connected.set(event.checked()),
                    }
                    span { "Skip accounts that are online" }
                }
                button {
                    class: "primary-button form-submit",
                    onclick: move |_| {
                        onsave.call(OverviewConfigFormData {
                            auto_upload_interval_minutes: interval_minutes().trim().to_string(),
                            auto_upload_jitter_minutes: jitter_minutes().trim().to_string(),
                            upload_on_launch: upload_on_launch(),
                            no_upload_while_connected: no_upload_while_connected(),
                        });
                    },
                    "Save Configuration"
                }
            }
        }
        section { class: "panel",
            div { class: "panel-header",
                h2 { "Sync Pipeline" }
                span { "PsyNet replay discovery" }
            }
            div { class: "activity-row",
                div { class: "status-dot" }
                p { "Auth, PsyNet match history, replay download, upload, and cache handling are wired behind the CLI/library APIs." }
            }
        }
    }
}

#[component]
fn HistoryView(
    history: Option<Result<Vec<HistoryRow>, String>>,
    message: String,
    backfill_running: bool,
    uploading: Option<ReplayUploadRequest>,
    failed_uploads: Vec<ReplayUploadRequest>,
    onrefresh: EventHandler<()>,
    onbackfill: EventHandler<()>,
    onupload: EventHandler<ReplayUploadRequest>,
) -> Element {
    let backfill_label = if backfill_running {
        "Backfilling..."
    } else {
        "Backfill Destinations"
    };

    rsx! {
        section { class: "panel history-panel",
            div { class: "panel-header",
                h2 { "RL API History" }
                div { class: "button-row",
                    button {
                        class: "secondary-button",
                        onclick: move |_| onrefresh.call(()),
                        "Refresh"
                    }
                    button {
                        class: "primary-button",
                        disabled: backfill_running,
                        onclick: move |_| {
                            if !backfill_running {
                                onbackfill.call(());
                            }
                        },
                        "{backfill_label}"
                    }
                }
            }
            if !message.is_empty() {
                div { class: "notice", "{message}" }
            }
            match history {
                None => rsx! {
                    p { class: "empty-state", "Loading current match history..." }
                },
                Some(Err(error)) => rsx! {
                    p { class: "empty-state error-state", "{error}" }
                },
                Some(Ok(rows)) => rsx! {
                    if rows.is_empty() {
                        p { class: "empty-state", "No current RL API history entries found." }
                    } else {
                        div { class: "history-table",
                            div { class: "history-row history-heading",
                                span { "Match" }
                                span { "Account" }
                                span { "When" }
                                span { "Arena" }
                                span { "Score" }
                                span { "Upload Destinations" }
                            }
                            for row in rows {
                                div { class: "history-row",
                                    span { class: "mono-cell", "{short_match_id(&row.match_id)}" }
                                    span { "{row.account}" }
                                    span { "{row.timestamp}" }
                                    span { "{row.map_name} / {row.playlist}" }
                                    span { "{row.score}" }
                                    div { class: "store-status-list",
                                        for destination in row.upload_destinations {
                                            div { class: "store-status",
                                                span { class: "store-name", "{destination.target_name}" }
                                                if destination.uploaded {
                                                    if let Some(location) = destination.location.clone() {
                                                        a {
                                                            class: "state-pill uploaded",
                                                            href: "{location}",
                                                            target: "_blank",
                                                            "Open"
                                                        }
                                                    } else if destination.upload_enabled {
                                                        button {
                                                            class: "compact-button",
                                                            title: "{upload_failure_reason(&failed_uploads, &destination.target_name, &row.match_id)}",
                                                            disabled: uploading.is_some(),
                                                            onclick: {
                                                                let target_name = destination.target_name.clone();
                                                                let match_id = row.match_id.clone();
                                                                move |_| {
                                                                    onupload.call(ReplayUploadRequest {
                                                                        target_name: target_name.clone(),
                                                                        match_id: match_id.clone(),
                                                                        reason: None,
                                                                    });
                                                                }
                                                            },
                                                            if uploading.as_ref().is_some_and(|request| is_same_upload(request, &destination.target_name, &row.match_id)) {
                                                                "Trying upload"
                                                            } else if failed_upload(&failed_uploads, &destination.target_name, &row.match_id).is_some() {
                                                                "Retry upload"
                                                            } else {
                                                                "Get link"
                                                            }
                                                        }
                                                    } else {
                                                        span {
                                                            class: "state-pill uploaded",
                                                            "{destination.state}"
                                                        }
                                                    }
                                                } else if !destination.upload_enabled {
                                                    span {
                                                        class: "state-pill missing",
                                                        "{destination.state}"
                                                    }
                                                } else {
                                                    button {
                                                        class: "compact-button",
                                                        title: "{upload_failure_reason(&failed_uploads, &destination.target_name, &row.match_id)}",
                                                        disabled: uploading.is_some(),
                                                        onclick: {
                                                            let target_name = destination.target_name.clone();
                                                            let match_id = row.match_id.clone();
                                                            move |_| {
                                                                onupload.call(ReplayUploadRequest {
                                                                    target_name: target_name.clone(),
                                                                    match_id: match_id.clone(),
                                                                    reason: None,
                                                                });
                                                            }
                                                        },
                                                        if uploading.as_ref().is_some_and(|request| is_same_upload(request, &destination.target_name, &row.match_id)) {
                                                            "Uploading"
                                                        } else if failed_upload(&failed_uploads, &destination.target_name, &row.match_id).is_some() {
                                                            "Retry"
                                                        } else {
                                                            "Upload"
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                },
            }
        }
    }
}

#[component]
fn AccountsView(
    summary: AppSummary,
    onadd: EventHandler<AccountFormData>,
    onremove: EventHandler<u32>,
) -> Element {
    let accounts = summary.accounts.clone();
    let account_count = accounts.len();
    let mut account_name = use_signal(String::new);
    let mut platform = use_signal(|| "epic".to_string());
    let mut sync_enabled = use_signal(|| true);
    let mut confirming_remove = use_signal(|| None::<u32>);
    let can_remove = account_count > 1;

    rsx! {
        section { class: "panel compact-panel",
            div { class: "panel-header",
                h2 { "Accounts" }
                span { "{account_count} configured" }
            }
            div { class: "account-form",
                label {
                    span { "Name" }
                    input {
                        value: "{account_name}",
                        placeholder: "Primary",
                        oninput: move |event| account_name.set(event.value()),
                    }
                }
                label {
                    span { "Platform" }
                    select {
                        value: "{platform}",
                        oninput: move |event| platform.set(event.value()),
                        option { value: "epic", "Epic" }
                        option { value: "steam", "Steam" }
                        option { value: "play_station", "PlayStation" }
                        option { value: "xbox", "Xbox" }
                        option { value: "nintendo", "Nintendo" }
                    }
                }
                label { class: "checkbox-field",
                    input {
                        r#type: "checkbox",
                        checked: sync_enabled(),
                        oninput: move |event| sync_enabled.set(event.checked()),
                    }
                    span { "Sync" }
                }
                button {
                    class: "primary-button form-submit",
                    onclick: move |_| {
                        onadd.call(AccountFormData {
                            name: account_name().trim().to_string(),
                            platform: platform(),
                            sync_enabled: sync_enabled(),
                        });
                        account_name.set(String::new());
                        platform.set("epic".to_string());
                        sync_enabled.set(true);
                    },
                    "Add Account"
                }
            }
            div { class: "account-list",
                for account in accounts {
                    article { class: "account-row",
                        div {
                            div { class: "row-title",
                                strong { "{account.name}" }
                                if account.selected {
                                    span { class: "badge", "Selected" }
                                }
                                if !account.sync_enabled {
                                    span { class: "badge muted", "Sync off" }
                                }
                            }
                            div { class: "row-meta",
                                span { "{account.platform}" }
                            }
                        }
                        div { class: "row-actions",
                            if confirming_remove() == Some(account.id) {
                                button {
                                    class: "danger-button",
                                    onclick: move |_| {
                                        onremove.call(account.id);
                                        confirming_remove.set(None);
                                    },
                                    "Confirm Remove"
                                }
                                button {
                                    class: "secondary-button",
                                    onclick: move |_| confirming_remove.set(None),
                                    "Cancel"
                                }
                            } else {
                                button {
                                    class: "secondary-button",
                                    disabled: !can_remove,
                                    onclick: move |_| {
                                        if can_remove {
                                            confirming_remove.set(Some(account.id));
                                        }
                                    },
                                    "Remove"
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn UploadDestinationsView(summary: AppSummary, onautoupload: EventHandler<bool>) -> Element {
    let auto_upload_value = if summary.auto_upload {
        "Enabled"
    } else {
        "Disabled"
    };
    let next_auto_upload = !summary.auto_upload;
    let auto_upload_action = if summary.auto_upload {
        "Disable auto upload"
    } else {
        "Enable auto upload"
    };
    let startup_behavior_label = "Window opens";
    let close_behavior_label = "Hides to tray when available";
    let upload_on_launch = if summary.upload_on_launch {
        "Run sync at launch"
    } else {
        "Wait for interval"
    };
    let connection_guard = if summary.no_upload_while_connected {
        "Skip accounts that are online"
    } else {
        "Upload even while account is online"
    };

    rsx! {
        div { class: "destinations-view",
            section { class: "panel destinations-panel",
                div { class: "panel-header",
                    h2 { "Upload Destinations" }
                    span { "{summary.upload_destination_count()} destinations" }
                }
                div { class: "destination-list",
                    for target in summary.upload_destinations {
                        article { class: "account-row destination-row",
                            div {
                                div { class: "row-title",
                                    strong { "{target.name}" }
                                }
                                div { class: "row-meta destination-meta",
                                    span { "{target.url}" }
                                    span { "{target.auth}" }
                                    if target.upload_enabled {
                                        span { "Enabled" }
                                    } else {
                                        span { "Disabled" }
                                    }
                                    if target.automatic {
                                        span { "Automatic" }
                                    } else {
                                        span { "Manual" }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            section { class: "panel destinations-panel",
                div { class: "panel-header",
                    h2 { "Upload Activity" }
                    span { "{summary.interval}" }
                }
                div { class: "activity-row main-action",
                    div { class: if summary.auto_upload { "status-dot" } else { "status-dot off" } }
                    p { "Auto upload: {auto_upload_value}" }
                    button {
                        class: "secondary-button",
                        onclick: move |_| onautoupload.call(next_auto_upload),
                        "{auto_upload_action}"
                    }
                }
                div { class: "activity-row main-action",
                    div { class: "status-dot" }
                    p { "Startup: {startup_behavior_label}" }
                }
                div { class: "activity-row main-action",
                    div { class: "status-dot" }
                    p { "Window close: {close_behavior_label}" }
                }
                dl { class: "details",
                    dt { "Cadence" }
                    dd { "{summary.interval}, jitter up to {summary.jitter}" }
                    dt { "Launch" }
                    dd { "{upload_on_launch}" }
                    dt { "Guard" }
                    dd { "{connection_guard}" }
                }
            }
        }
    }
}

#[cfg(all(
    feature = "desktop",
    not(any(target_os = "ios", target_os = "android"))
))]
#[derive(Clone, Debug)]
enum TrayCommand {
    ShowWindow,
    ToggleWindow,
    SyncNow,
    RefreshHistory,
    Retry(ReplayUploadRequest),
    Quit,
}

#[cfg(all(
    feature = "desktop",
    not(any(target_os = "ios", target_os = "android"))
))]
struct TrayState {
    sender: std::sync::mpsc::Sender<TrayThreadMessage>,
}

#[cfg(all(
    feature = "desktop",
    not(any(target_os = "ios", target_os = "android"))
))]
impl Drop for TrayState {
    fn drop(&mut self) {
        let _ = self.sender.send(TrayThreadMessage::Shutdown);
    }
}

#[cfg(all(
    feature = "desktop",
    not(any(target_os = "ios", target_os = "android"))
))]
enum TrayThreadMessage {
    Update(Box<TrayUpdate>),
    Shutdown,
}

#[cfg(all(
    feature = "desktop",
    not(any(target_os = "ios", target_os = "android"))
))]
struct TrayUpdate {
    summary: AppSummary,
    history: Option<Result<Vec<HistoryRow>, String>>,
    sync_run: SyncRunState,
    failed_uploads: Vec<ReplayUploadRequest>,
}

#[cfg(all(
    feature = "desktop",
    not(any(target_os = "ios", target_os = "android"))
))]
#[component]
fn DesktopTrayBridge(
    summary: AppSummary,
    history: Option<Result<Vec<HistoryRow>, String>>,
    sync_run: SyncRunState,
    failed_uploads: Vec<ReplayUploadRequest>,
    onsync: EventHandler<()>,
    onrefreshhistory: EventHandler<()>,
    onretry: EventHandler<ReplayUploadRequest>,
) -> Element {
    use dioxus::desktop::WindowCloseBehaviour;
    use futures_util::StreamExt;

    let mut tray_state = use_signal(|| None);
    let command_handler = use_coroutine(
        move |mut receiver: UnboundedReceiver<TrayCommand>| async move {
            while let Some(command) = receiver.next().await {
                match command {
                    TrayCommand::ShowWindow => show_window(),
                    TrayCommand::ToggleWindow => toggle_window_visibility(),
                    TrayCommand::SyncNow => {
                        show_window();
                        onsync.call(());
                    }
                    TrayCommand::RefreshHistory => onrefreshhistory.call(()),
                    TrayCommand::Retry(request) => onretry.call(request),
                    TrayCommand::Quit => {
                        quit_application(tray_state);
                        return;
                    }
                }
            }
        },
    );

    use_hook(move || {
        let state = create_tray_state(command_handler.tx());
        let behaviour = if state.is_some() {
            WindowCloseBehaviour::WindowHides
        } else {
            WindowCloseBehaviour::WindowCloses
        };
        dioxus::desktop::window().set_close_behavior(behaviour);
        tray_state.set(state);
    });

    use_effect(use_reactive!(
        |summary, history, sync_run, failed_uploads| {
            if let Some(tray_state) = tray_state.read().as_ref() {
                update_tray_state(
                    tray_state,
                    summary.clone(),
                    history.clone(),
                    sync_run.clone(),
                    failed_uploads.clone(),
                );
            }
        }
    ));

    rsx! {}
}

#[cfg(all(
    feature = "desktop",
    not(any(target_os = "ios", target_os = "android"))
))]
fn create_tray_state(sender: UnboundedSender<TrayCommand>) -> Option<TrayState> {
    match create_tray_state_inner(sender) {
        Ok(state) => Some(state),
        Err(error) => {
            eprintln!("Failed to initialize rlru tray icon: {error}");
            None
        }
    }
}

#[cfg(all(
    feature = "desktop",
    not(any(target_os = "ios", target_os = "android"))
))]
fn create_tray_state_inner(sender: UnboundedSender<TrayCommand>) -> Result<TrayState, String> {
    use ksni::blocking::TrayMethods;

    let tray = RlruTrayItem::new(sender);
    let (thread_sender, thread_receiver) = std::sync::mpsc::channel();
    let (handle_sender, handle_receiver) = std::sync::mpsc::channel();

    std::thread::spawn(move || {
        let result = tray
            .spawn()
            .map_err(|error| format!("failed to build rlru tray icon: {error}"));
        match result {
            Ok(handle) => {
                let _ = handle_sender.send(Ok(()));
                run_tray_thread(handle, thread_receiver);
            }
            Err(error) => {
                let _ = handle_sender.send(Err(error));
            }
        }
    });

    handle_receiver
        .recv()
        .map_err(|error| format!("failed to start rlru tray thread: {error}"))??;

    Ok(TrayState {
        sender: thread_sender,
    })
}

#[cfg(all(
    feature = "desktop",
    not(any(target_os = "ios", target_os = "android"))
))]
fn run_tray_thread(
    handle: ksni::blocking::Handle<RlruTrayItem>,
    receiver: std::sync::mpsc::Receiver<TrayThreadMessage>,
) {
    for message in receiver {
        match message {
            TrayThreadMessage::Update(update) => {
                let _ = handle.update(move |tray| {
                    tray.summary = Some(update.summary);
                    tray.history = update.history;
                    tray.sync_run = update.sync_run;
                    tray.failed_uploads = update.failed_uploads;
                });
            }
            TrayThreadMessage::Shutdown => {
                handle.shutdown().wait();
                return;
            }
        }
    }
}

#[cfg(all(
    feature = "desktop",
    not(any(target_os = "ios", target_os = "android"))
))]
fn update_tray_state(
    state: &TrayState,
    summary: AppSummary,
    history: Option<Result<Vec<HistoryRow>, String>>,
    sync_run: SyncRunState,
    failed_uploads: Vec<ReplayUploadRequest>,
) {
    let _ = state
        .sender
        .send(TrayThreadMessage::Update(Box::new(TrayUpdate {
            summary,
            history,
            sync_run,
            failed_uploads,
        })));
}

#[cfg(all(
    feature = "desktop",
    not(any(target_os = "ios", target_os = "android"))
))]
fn disabled_item<T>(label: impl Into<String>) -> ksni::menu::MenuItem<T> {
    ksni::menu::StandardItem {
        label: label.into(),
        enabled: false,
        ..Default::default()
    }
    .into()
}

#[cfg(all(
    feature = "desktop",
    not(any(target_os = "ios", target_os = "android"))
))]
fn action_item(
    label: &str,
    command: TrayCommand,
    sender: &UnboundedSender<TrayCommand>,
    enabled: bool,
) -> ksni::menu::MenuItem<RlruTrayItem> {
    let sender = sender.clone();
    ksni::menu::StandardItem {
        label: label.to_string(),
        enabled,
        activate: Box::new(move |_| {
            let _ = sender.unbounded_send(command.clone());
        }),
        ..Default::default()
    }
    .into()
}

#[cfg(all(
    feature = "desktop",
    not(any(target_os = "ios", target_os = "android"))
))]
fn submenu(
    label: impl Into<String>,
    items: Vec<ksni::menu::MenuItem<RlruTrayItem>>,
) -> ksni::menu::MenuItem<RlruTrayItem> {
    ksni::menu::SubMenu {
        label: label.into(),
        submenu: items,
        ..Default::default()
    }
    .into()
}

#[cfg(all(
    feature = "desktop",
    not(any(target_os = "ios", target_os = "android"))
))]
fn history_menu_items(
    history: Option<&Result<Vec<HistoryRow>, String>>,
    failed_uploads: &[ReplayUploadRequest],
) -> Vec<ksni::menu::MenuItem<RlruTrayItem>> {
    match history {
        None => vec![disabled_item("Loading current history")],
        Some(Err(error)) => vec![disabled_item(format!("History unavailable: {error}"))],
        Some(Ok(rows)) if rows.is_empty() => vec![disabled_item("No current history entries")],
        Some(Ok(rows)) => rows
            .iter()
            .take(8)
            .map(|row| {
                let mut row_items = vec![
                    disabled_item(format!("Account: {}", row.account)),
                    disabled_item(format!("When: {}", row.timestamp)),
                ];
                row_items.extend(row.upload_destinations.iter().map(|destination| {
                    let state = if let Some(failure) =
                        failed_upload(failed_uploads, &destination.target_name, &row.match_id)
                    {
                        failure.reason.as_deref().unwrap_or("Failed")
                    } else {
                        destination.state.as_str()
                    };
                    disabled_item(format!("{}: {}", destination.target_name, state))
                }));

                submenu(
                    format!(
                        "{} - {} - {}",
                        short_match_id(&row.match_id),
                        row.map_name,
                        row.score
                    ),
                    row_items,
                )
            })
            .collect(),
    }
}

#[cfg(all(
    feature = "desktop",
    not(any(target_os = "ios", target_os = "android"))
))]
fn png_to_argb32(png_data: &[u8]) -> ksni::Icon {
    let image = image::load_from_memory_with_format(png_data, image::ImageFormat::Png)
        .expect("embedded PNG is valid")
        .into_rgba8();
    let data = image
        .pixels()
        .flat_map(|pixel| [pixel[3], pixel[0], pixel[1], pixel[2]])
        .collect();
    ksni::Icon {
        width: image.width() as i32,
        height: image.height() as i32,
        data,
    }
}

#[cfg(all(
    feature = "desktop",
    not(any(target_os = "ios", target_os = "android"))
))]
fn load_icon_set() -> Vec<ksni::Icon> {
    vec![png_to_argb32(APP_ICON_PNG)]
}

#[cfg(all(
    feature = "desktop",
    not(any(target_os = "ios", target_os = "android"))
))]
struct RlruTrayItem {
    summary: Option<AppSummary>,
    history: Option<Result<Vec<HistoryRow>, String>>,
    sync_run: SyncRunState,
    failed_uploads: Vec<ReplayUploadRequest>,
    sender: UnboundedSender<TrayCommand>,
    icons: Vec<ksni::Icon>,
}

#[cfg(all(
    feature = "desktop",
    not(any(target_os = "ios", target_os = "android"))
))]
impl RlruTrayItem {
    fn new(sender: UnboundedSender<TrayCommand>) -> Self {
        Self {
            summary: None,
            history: None,
            sync_run: SyncRunState::default(),
            failed_uploads: Vec::new(),
            sender,
            icons: load_icon_set(),
        }
    }
}

#[cfg(all(
    feature = "desktop",
    not(any(target_os = "ios", target_os = "android"))
))]
impl ksni::Tray for RlruTrayItem {
    const MENU_ON_ACTIVATE: bool = false;

    fn id(&self) -> String {
        "rlru-dioxus".to_string()
    }

    fn title(&self) -> String {
        "rlru".to_string()
    }

    fn icon_name(&self) -> String {
        String::new()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        self.icons.clone()
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        let description = self
            .summary
            .as_ref()
            .map(|summary| tray_tooltip(summary, &self.sync_run, self.failed_uploads.len()))
            .unwrap_or_else(|| "rlru\nTray data loading".to_string());

        ksni::ToolTip {
            title: "rlru".to_string(),
            description,
            ..Default::default()
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        let _ = self.sender.unbounded_send(TrayCommand::ToggleWindow);
    }

    fn menu(&self) -> Vec<ksni::menu::MenuItem<Self>> {
        let mut items = Vec::new();
        items.push(disabled_item("rlru"));
        items.push(ksni::menu::MenuItem::Separator);

        if let Some(summary) = self.summary.as_ref() {
            items.push(disabled_item(tray_sync_label(&self.sync_run)));
            items.push(disabled_item(format!(
                "Auto upload: {}, {}",
                auto_upload_label(summary),
                summary.interval
            )));
        } else {
            items.push(disabled_item("Tray data loading"));
        }

        items.push(ksni::menu::MenuItem::Separator);
        items.push(submenu(
            "History",
            history_menu_items(self.history.as_ref(), &self.failed_uploads),
        ));

        if !self.failed_uploads.is_empty() {
            let failures = self
                .failed_uploads
                .iter()
                .cloned()
                .map(|request| {
                    action_item(
                        &format_failed_upload_retry_label(&request),
                        TrayCommand::Retry(request),
                        &self.sender,
                        true,
                    )
                })
                .collect();
            items.push(submenu(
                format!("Failed Uploads ({})", self.failed_uploads.len()),
                failures,
            ));
        }

        items.extend([
            ksni::menu::MenuItem::Separator,
            action_item(
                "Sync Now",
                TrayCommand::SyncNow,
                &self.sender,
                !self.sync_run.running,
            ),
            action_item(
                "Refresh History",
                TrayCommand::RefreshHistory,
                &self.sender,
                true,
            ),
            action_item("Open App", TrayCommand::ShowWindow, &self.sender, true),
            action_item(
                "Show/Hide Window",
                TrayCommand::ToggleWindow,
                &self.sender,
                true,
            ),
            ksni::menu::MenuItem::Separator,
            action_item("Quit", TrayCommand::Quit, &self.sender, true),
        ]);

        items
    }
}

#[cfg(not(all(
    feature = "desktop",
    not(any(target_os = "ios", target_os = "android"))
)))]
#[component]
fn DesktopTrayBridge(
    summary: AppSummary,
    history: Option<Result<Vec<HistoryRow>, String>>,
    sync_run: SyncRunState,
    failed_uploads: Vec<ReplayUploadRequest>,
    onsync: EventHandler<()>,
    onrefreshhistory: EventHandler<()>,
    onretry: EventHandler<ReplayUploadRequest>,
) -> Element {
    let _ = (
        summary,
        history,
        sync_run,
        failed_uploads,
        onsync,
        onrefreshhistory,
        onretry,
    );
    rsx! {}
}

#[cfg(all(
    feature = "desktop",
    not(any(target_os = "ios", target_os = "android"))
))]
fn show_window() {
    let win = dioxus::desktop::window();
    restore_desktop_window_handle(win.window.clone());
}

#[cfg(all(
    feature = "desktop",
    not(any(target_os = "ios", target_os = "android"))
))]
fn toggle_window_visibility() {
    let win = dioxus::desktop::window();
    if win.window.is_visible() {
        win.set_visible(false);
    } else {
        restore_desktop_window_handle(win.window.clone());
    }
}

#[cfg(all(
    feature = "desktop",
    not(any(target_os = "ios", target_os = "android"))
))]
fn quit_application(mut tray_state: Signal<Option<TrayState>>) {
    use dioxus::desktop::WindowCloseBehaviour;

    if let Some(tray_state) = tray_state.write().take() {
        let _ = tray_state.sender.send(TrayThreadMessage::Shutdown);
    }

    cleanup_desktop_instance_socket();

    let win = dioxus::desktop::window();
    win.set_close_behavior(WindowCloseBehaviour::WindowCloses);
    win.close();

    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_millis(300));
        std::process::exit(0);
    });
}

#[component]
fn Metric(label: String, value: String) -> Element {
    rsx! {
        article { class: "metric",
            small { "{label}" }
            strong { "{value}" }
        }
    }
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, PartialEq)]
struct AppSummary {
    config_path: String,
    accounts: Vec<AccountSummary>,
    upload_destinations: Vec<UploadDestinationSummary>,
    auto_upload: bool,
    upload_on_launch: bool,
    no_upload_while_connected: bool,
    selected_account: Option<String>,
    selected_upload_destination: Option<String>,
    auto_upload_interval_minutes: u64,
    auto_upload_jitter_minutes: u64,
    interval: String,
    jitter: String,
    status: String,
}

#[cfg(target_arch = "wasm32")]
impl AppSummary {
    fn account_count(&self) -> usize {
        self.accounts.len()
    }

    fn upload_destination_count(&self) -> usize {
        self.upload_destinations.len()
    }
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, PartialEq)]
struct AccountSummary {
    id: u32,
    name: String,
    platform: String,
    sync_enabled: bool,
    selected: bool,
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, PartialEq, Eq)]
struct AccountFormData {
    name: String,
    platform: String,
    sync_enabled: bool,
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, PartialEq, Eq)]
struct OverviewConfigFormData {
    auto_upload_interval_minutes: String,
    auto_upload_jitter_minutes: String,
    upload_on_launch: bool,
    no_upload_while_connected: bool,
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, PartialEq)]
struct UploadDestinationSummary {
    name: String,
    url: String,
    upload_enabled: bool,
    automatic: bool,
    auth: String,
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, PartialEq, Eq)]
struct HistoryRow {
    account: String,
    match_id: String,
    timestamp: String,
    map_name: String,
    playlist: String,
    score: String,
    upload_destinations: Vec<HistoryUploadDestination>,
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, PartialEq, Eq)]
struct HistoryUploadDestination {
    target_name: String,
    state: String,
    uploaded: bool,
    upload_enabled: bool,
    location: Option<String>,
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, PartialEq, Eq)]
struct ReplayUploadRequest {
    target_name: String,
    match_id: String,
    reason: Option<String>,
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct SyncRunState {
    running: bool,
    last_started_at: Option<String>,
    last_completed_at: Option<String>,
    last_summary: Option<BackfillSummary>,
    last_error: Option<String>,
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, PartialEq, Eq)]
struct BackfillSummary {
    uploaded: usize,
    duplicates: usize,
    cached: usize,
    failed: usize,
    failed_match_ids: Vec<String>,
    failed_uploads: Vec<ReplayUploadRequest>,
}

#[cfg(target_arch = "wasm32")]
fn now_label() -> String {
    "now".to_string()
}

#[cfg(target_arch = "wasm32")]
fn short_match_id(match_id: &str) -> &str {
    match_id.get(..8).unwrap_or(match_id)
}

#[cfg(target_arch = "wasm32")]
fn format_backfill_message(message: String, failed_uploads: &[ReplayUploadRequest]) -> String {
    if failed_uploads.is_empty() {
        message
    } else {
        format!(
            "{message}; first issue: {}",
            format_failed_upload(&failed_uploads[0])
        )
    }
}

#[cfg(target_arch = "wasm32")]
fn dedupe_upload_requests(requests: Vec<ReplayUploadRequest>) -> Vec<ReplayUploadRequest> {
    let mut deduped = Vec::new();
    for request in requests {
        upsert_failed_upload(&mut deduped, request);
    }
    deduped
}

#[cfg(target_arch = "wasm32")]
fn upsert_failed_upload(
    failed_uploads: &mut Vec<ReplayUploadRequest>,
    request: ReplayUploadRequest,
) {
    if let Some(existing) = failed_uploads
        .iter_mut()
        .find(|failure| is_same_upload_request(failure, &request))
    {
        *existing = request;
    } else {
        failed_uploads.push(request);
    }
}

#[cfg(target_arch = "wasm32")]
fn is_same_upload_request(left: &ReplayUploadRequest, right: &ReplayUploadRequest) -> bool {
    is_same_upload(left, &right.target_name, &right.match_id)
}

#[cfg(target_arch = "wasm32")]
fn is_same_upload(request: &ReplayUploadRequest, target_name: &str, match_id: &str) -> bool {
    request.target_name == target_name && request.match_id == match_id
}

#[cfg(target_arch = "wasm32")]
fn failed_upload<'a>(
    failed_uploads: &'a [ReplayUploadRequest],
    target_name: &str,
    match_id: &str,
) -> Option<&'a ReplayUploadRequest> {
    failed_uploads
        .iter()
        .find(|failure| is_same_upload(failure, target_name, match_id))
}

#[cfg(target_arch = "wasm32")]
fn upload_failure_reason(
    failed_uploads: &[ReplayUploadRequest],
    target_name: &str,
    match_id: &str,
) -> String {
    failed_upload(failed_uploads, target_name, match_id)
        .and_then(|failure| failure.reason.clone())
        .unwrap_or_default()
}

#[cfg(target_arch = "wasm32")]
fn format_failed_upload(failure: &ReplayUploadRequest) -> String {
    let base = format!(
        "{} to {}",
        short_match_id(&failure.match_id),
        failure.target_name
    );
    match &failure.reason {
        Some(reason) => format!("{base}: {reason}"),
        None => base,
    }
}

#[cfg(target_arch = "wasm32")]
async fn load_history() -> Result<Vec<HistoryRow>, String> {
    Ok(vec![
        HistoryRow {
            account: "colonelpanic8".to_string(),
            match_id: "4E8409F8A8F4431DBF2412B30F2461B5".to_string(),
            timestamp: "1700000000".to_string(),
            map_name: "DFH Stadium".to_string(),
            playlist: "13".to_string(),
            score: "3-2".to_string(),
            upload_destinations: vec![HistoryUploadDestination {
                target_name: "Rocket Sense".to_string(),
                state: "Uploaded".to_string(),
                uploaded: true,
                upload_enabled: true,
                location: Some("https://rocket-sense.duckdns.org/replays/demo".to_string()),
            }],
        },
        HistoryRow {
            account: "colonelpanic8".to_string(),
            match_id: "F90812E5EFDA4CC4AC7903596F02E6AB".to_string(),
            timestamp: "1699999000".to_string(),
            map_name: "Mannfield".to_string(),
            playlist: "13".to_string(),
            score: "1-4".to_string(),
            upload_destinations: vec![HistoryUploadDestination {
                target_name: "Rocket Sense".to_string(),
                state: "Not uploaded".to_string(),
                uploaded: false,
                upload_enabled: true,
                location: None,
            }],
        },
    ])
}

#[cfg(target_arch = "wasm32")]
async fn backfill_upload_destinations() -> Result<BackfillSummary, String> {
    Ok(BackfillSummary {
        uploaded: 1,
        duplicates: 0,
        cached: 1,
        failed: 0,
        failed_match_ids: Vec::new(),
        failed_uploads: Vec::new(),
    })
}

#[cfg(target_arch = "wasm32")]
async fn upload_history_replay(_request: ReplayUploadRequest) -> Result<BackfillSummary, String> {
    Ok(BackfillSummary {
        uploaded: 1,
        duplicates: 0,
        cached: 0,
        failed: 0,
        failed_match_ids: Vec::new(),
        failed_uploads: Vec::new(),
    })
}

#[cfg(target_arch = "wasm32")]
fn load_summary() -> AppSummary {
    AppSummary {
        config_path: "Browser preview uses default local config shape".to_string(),
        accounts: vec![AccountSummary {
            id: 1,
            name: "colonelpanic8".to_string(),
            platform: "Epic".to_string(),
            sync_enabled: true,
            selected: true,
        }],
        upload_destinations: vec![UploadDestinationSummary {
            name: "Rocket Sense".to_string(),
            url: "https://rocket-sense.duckdns.org/api/v1".to_string(),
            upload_enabled: true,
            automatic: true,
            auth: "Bearer env token".to_string(),
        }],
        auto_upload: true,
        upload_on_launch: false,
        no_upload_while_connected: false,
        selected_account: Some("colonelpanic8".to_string()),
        selected_upload_destination: Some("Rocket Sense".to_string()),
        auto_upload_interval_minutes: 45,
        auto_upload_jitter_minutes: 15,
        interval: "Every 45 minutes".to_string(),
        jitter: "15 minutes".to_string(),
        status: "Ready for auth, sync, and uploader runs".to_string(),
    }
}

#[cfg(target_arch = "wasm32")]
fn add_account(input: AccountFormData) -> Result<AppSummary, String> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err("Account name is required".to_string());
    }

    let mut summary = load_summary();
    let next_id = summary
        .accounts
        .iter()
        .map(|account| account.id)
        .max()
        .unwrap_or(0)
        .saturating_add(1);
    summary.accounts.push(AccountSummary {
        id: next_id,
        name: name.to_string(),
        platform: platform_preview_label(&input.platform).to_string(),
        sync_enabled: input.sync_enabled,
        selected: false,
    });
    Ok(summary)
}

#[cfg(target_arch = "wasm32")]
fn remove_account(account_id: u32) -> Result<AppSummary, String> {
    let mut summary = load_summary();
    summary.accounts.retain(|account| account.id != account_id);
    Ok(summary)
}

#[cfg(target_arch = "wasm32")]
fn platform_preview_label(value: &str) -> &'static str {
    match value {
        "steam" => "Steam",
        "play_station" => "PlayStation",
        "xbox" => "Xbox",
        "nintendo" => "Nintendo",
        _ => "Epic",
    }
}

#[cfg(target_arch = "wasm32")]
fn save_auto_upload(enabled: bool) -> Result<AppSummary, String> {
    let mut summary = load_summary();
    summary.auto_upload = enabled;
    Ok(summary)
}

#[cfg(target_arch = "wasm32")]
fn save_overview_config(input: OverviewConfigFormData) -> Result<AppSummary, String> {
    let interval_minutes = input
        .auto_upload_interval_minutes
        .trim()
        .parse::<u64>()
        .map_err(|_| "sync interval must be a whole number of minutes".to_string())?;
    let jitter_minutes = input
        .auto_upload_jitter_minutes
        .trim()
        .parse::<u64>()
        .map_err(|_| "jitter max must be a whole number of minutes".to_string())?;
    if interval_minutes == 0 {
        return Err("sync interval must be at least 1 minute".to_string());
    }
    if jitter_minutes > interval_minutes {
        return Err("auto_upload_jitter_max cannot exceed auto_upload_interval".to_string());
    }

    let mut summary = load_summary();
    summary.auto_upload_interval_minutes = interval_minutes;
    summary.auto_upload_jitter_minutes = jitter_minutes;
    summary.upload_on_launch = input.upload_on_launch;
    summary.no_upload_while_connected = input.no_upload_while_connected;
    summary.interval = format!("Every {interval_minutes} minutes");
    summary.jitter = format!("{jitter_minutes} minutes");
    Ok(summary)
}
