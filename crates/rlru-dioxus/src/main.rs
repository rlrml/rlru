use dioxus::prelude::*;

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
    Activity,
}

impl ActiveView {
    const ALL: [Self; 5] = [
        Self::Overview,
        Self::History,
        Self::Accounts,
        Self::UploadDestinations,
        Self::Activity,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::History => "History",
            Self::Accounts => "Accounts",
            Self::UploadDestinations => "Upload Destinations",
            Self::Activity => "Activity",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::Overview => "Local auth, typed config, replay upload destinations",
            Self::History => "Current RL API matches and upload destination state",
            Self::Accounts => "Configured Rocket League account credentials",
            Self::UploadDestinations => "Replay upload destinations and cache state",
            Self::Activity => "Sync and uploader pipeline status",
        }
    }
}

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

impl AppSummary {
    fn account_count(&self) -> usize {
        self.accounts.len()
    }

    fn upload_destination_count(&self) -> usize {
        self.upload_destinations.len()
    }
}

#[derive(Clone, Debug, PartialEq)]
struct AccountSummary {
    id: u32,
    name: String,
    profile_id: u32,
    platform: String,
    unused: bool,
    selected: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct AccountFormData {
    name: String,
    profile_id: String,
    platform: String,
    unused: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct OverviewConfigFormData {
    auto_upload_interval_minutes: String,
    auto_upload_jitter_minutes: String,
    upload_on_launch: bool,
    no_upload_while_connected: bool,
}

#[derive(Clone, Debug, PartialEq)]
struct UploadDestinationSummary {
    name: String,
    url: String,
    primary: bool,
    predefined: bool,
    upload_enabled: bool,
    auth: String,
    selected: bool,
}

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

#[derive(Clone, Debug, PartialEq, Eq)]
struct HistoryUploadDestination {
    target_name: String,
    state: String,
    uploaded: bool,
    upload_enabled: bool,
    location: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ReplayUploadRequest {
    target_name: String,
    match_id: String,
    reason: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct SyncRunState {
    running: bool,
    last_started_at: Option<String>,
    last_completed_at: Option<String>,
    last_summary: Option<BackfillSummary>,
    last_error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct BackfillSummary {
    uploaded: usize,
    duplicates: usize,
    cached: usize,
    failed: usize,
    failed_match_ids: Vec<String>,
    failed_uploads: Vec<ReplayUploadRequest>,
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
    let mut history = use_resource(move || async move {
        if active_view() == ActiveView::History {
            load_history().await
        } else {
            Ok(Vec::new())
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
    let current_history = history.cloned();
    let history_status = history_message();
    let is_backfill_running = backfill_running();
    let current_uploading_replay = uploading_replay();
    let current_sync_run = sync_run();
    let current_failed_uploads = failed_uploads();

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
                            history_message.set(crate::format_backfill_message(
                                format!(
                                    "Sync complete: {} uploaded, {} duplicates, {} cached, {} failed",
                                    run_summary.uploaded,
                                    run_summary.duplicates,
                                    run_summary.cached,
                                    run_summary.failed
                                ),
                                &run_summary.failed_uploads,
                            ));
                            history.restart();
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
                history.restart();
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
                            history_message.set(crate::format_backfill_message(
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
                            history.restart();
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
                                history.restart();
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
                                            history_message.set(crate::format_backfill_message(
                                                format!(
                                                "Backfill complete: {} uploaded, {} duplicates, {} cached, {} failed",
                                                run_summary.uploaded,
                                                run_summary.duplicates,
                                                run_summary.cached,
                                                run_summary.failed
                                                ),
                                                &run_summary.failed_uploads,
                                            ));
                                            history.restart();
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
                                            history_message.set(crate::format_backfill_message(
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
                                            history.restart();
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
                        UploadDestinationsView { summary: current_summary }
                    },
                    ActiveView::Activity => rsx! {
                        ActivityView {
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
    let next_profile_id_default = next_profile_id(&accounts).to_string();
    let mut account_name = use_signal(String::new);
    let mut profile_id = use_signal(|| next_profile_id_default);
    let mut platform = use_signal(|| "epic".to_string());
    let mut unused = use_signal(|| false);
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
                    span { "Profile" }
                    input {
                        r#type: "number",
                        min: "0",
                        value: "{profile_id}",
                        oninput: move |event| profile_id.set(event.value()),
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
                        checked: unused(),
                        oninput: move |event| unused.set(event.checked()),
                    }
                    span { "Unused" }
                }
                button {
                    class: "primary-button form-submit",
                    onclick: move |_| {
                        onadd.call(AccountFormData {
                            name: account_name().trim().to_string(),
                            profile_id: profile_id().trim().to_string(),
                            platform: platform(),
                            unused: unused(),
                        });
                        account_name.set(String::new());
                        let next_profile_id = profile_id()
                            .parse::<u32>()
                            .unwrap_or(0)
                            .saturating_add(1)
                            .to_string();
                        profile_id.set(next_profile_id);
                        platform.set("epic".to_string());
                        unused.set(false);
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
                                if account.unused {
                                    span { class: "badge muted", "Unused" }
                                }
                            }
                            div { class: "row-meta",
                                span { "{account.platform}" }
                                span { "Profile {account.profile_id}" }
                                span { "ID {account.id}" }
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

fn next_profile_id(accounts: &[AccountSummary]) -> u32 {
    accounts
        .iter()
        .map(|account| account.profile_id)
        .max()
        .unwrap_or(0)
        .saturating_add(1)
}

#[component]
fn UploadDestinationsView(summary: AppSummary) -> Element {
    rsx! {
        section { class: "panel compact-panel",
            div { class: "panel-header",
                h2 { "Upload Destinations" }
                span { "{summary.upload_destination_count()} destinations" }
            }
            div { class: "account-list",
                for target in summary.upload_destinations {
                    article { class: "account-row",
                        div {
                            div { class: "row-title",
                                strong { "{target.name}" }
                                if target.selected {
                                    span { class: "badge", "Selected" }
                                }
                                if target.primary {
                                    span { class: "badge", "Primary" }
                                }
                                if target.predefined {
                                    span { class: "badge muted", "Built-in" }
                                }
                            }
                            div { class: "row-meta",
                                span { "{target.url}" }
                                span { "{target.auth}" }
                                if target.upload_enabled {
                                    span { "Uploads enabled" }
                                } else {
                                    span { "Uploads disabled" }
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
fn ActivityView(summary: AppSummary, onautoupload: EventHandler<bool>) -> Element {
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
        section { class: "panel compact-panel",
            div { class: "panel-header",
                h2 { "Activity" }
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
    const MENU_ON_ACTIVATE: bool = true;

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
        let _ = self.sender.unbounded_send(TrayCommand::ShowWindow);
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

fn short_match_id(match_id: &str) -> &str {
    match_id.get(..8).unwrap_or(match_id)
}

#[cfg(not(target_arch = "wasm32"))]
fn now_label() -> String {
    chrono::Local::now()
        .format("%Y-%m-%d %H:%M:%S %Z")
        .to_string()
}

#[cfg(target_arch = "wasm32")]
fn now_label() -> String {
    "now".to_string()
}

fn format_backfill_message(message: String, failed_uploads: &[ReplayUploadRequest]) -> String {
    if failed_uploads.is_empty() {
        message
    } else {
        let suffix = if failed_uploads.len() == 1 {
            String::new()
        } else {
            format!("; {} total blocked/failed uploads", failed_uploads.len())
        };
        format!(
            "{message}; first issue: {}{suffix}",
            format_failed_upload(&failed_uploads[0])
        )
    }
}

fn dedupe_upload_requests(requests: Vec<ReplayUploadRequest>) -> Vec<ReplayUploadRequest> {
    let mut deduped = Vec::new();
    for request in requests {
        upsert_failed_upload(&mut deduped, request);
    }
    deduped
}

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

fn is_same_upload_request(left: &ReplayUploadRequest, right: &ReplayUploadRequest) -> bool {
    is_same_upload(left, &right.target_name, &right.match_id)
}

fn is_same_upload(request: &ReplayUploadRequest, target_name: &str, match_id: &str) -> bool {
    request.target_name == target_name && request.match_id == match_id
}

fn failed_upload<'a>(
    failed_uploads: &'a [ReplayUploadRequest],
    target_name: &str,
    match_id: &str,
) -> Option<&'a ReplayUploadRequest> {
    failed_uploads
        .iter()
        .find(|failure| is_same_upload(failure, target_name, match_id))
}

fn upload_failure_reason(
    failed_uploads: &[ReplayUploadRequest],
    target_name: &str,
    match_id: &str,
) -> String {
    failed_upload(failed_uploads, target_name, match_id)
        .and_then(|failure| failure.reason.clone())
        .unwrap_or_default()
}

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

#[cfg(all(
    feature = "desktop",
    not(any(target_os = "ios", target_os = "android"))
))]
fn format_failed_upload_retry_label(failure: &ReplayUploadRequest) -> String {
    format!("Retry {}", format_failed_upload(failure))
}

#[cfg(all(
    feature = "desktop",
    not(any(target_os = "ios", target_os = "android"))
))]
fn auto_upload_label(summary: &AppSummary) -> &'static str {
    if summary.auto_upload {
        "enabled"
    } else {
        "disabled"
    }
}

#[cfg(all(
    feature = "desktop",
    not(any(target_os = "ios", target_os = "android"))
))]
fn tray_sync_label(sync_run: &SyncRunState) -> String {
    if sync_run.running {
        return sync_run
            .last_started_at
            .as_ref()
            .map(|started| format!("Sync running since {started}"))
            .unwrap_or_else(|| "Sync running".to_string());
    }

    if let Some(error) = &sync_run.last_error {
        return sync_run
            .last_completed_at
            .as_ref()
            .map(|completed| format!("Last sync failed at {completed}: {error}"))
            .unwrap_or_else(|| format!("Last sync failed: {error}"));
    }

    match (&sync_run.last_completed_at, &sync_run.last_summary) {
        (Some(completed), Some(summary)) => format!(
            "Last sync {completed}: {} uploaded, {} duplicate, {} cached, {} failed",
            summary.uploaded, summary.duplicates, summary.cached, summary.failed
        ),
        (Some(completed), None) => format!("Last sync {completed}"),
        _ => "No sync run yet".to_string(),
    }
}

#[cfg(all(
    feature = "desktop",
    not(any(target_os = "ios", target_os = "android"))
))]
#[cfg(all(
    feature = "desktop",
    not(any(target_os = "ios", target_os = "android"))
))]
fn tray_tooltip(summary: &AppSummary, sync_run: &SyncRunState, failed_count: usize) -> String {
    format!(
        "rlru\n{}\nAuto upload: {}, {}\nFailed uploads: {}",
        tray_sync_label(sync_run),
        auto_upload_label(summary),
        summary.interval,
        failed_count
    )
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

#[cfg(not(target_arch = "wasm32"))]
async fn load_history() -> Result<Vec<HistoryRow>, String> {
    use rlru::paths::AppPaths;
    use rlru::sync::SyncService;
    use rlru::Config;

    let paths = AppPaths::discover().map_err(|error| error.to_string())?;
    let config_path = paths.config_file();
    let config = Config::load_or_default(&config_path).map_err(|error| error.to_string())?;
    let service = SyncService::new(paths, config);
    let entries = service
        .current_history(None)
        .await
        .map_err(|error| error.to_string())?;

    Ok(entries
        .into_iter()
        .map(|entry| {
            let upload_destinations = entry
                .upload_states
                .into_iter()
                .map(|state| {
                    let label = if state.cached {
                        "Uploaded"
                    } else if !state.upload_enabled {
                        "Disabled"
                    } else {
                        "Not uploaded"
                    };
                    HistoryUploadDestination {
                        target_name: state.target_name,
                        state: label.to_string(),
                        uploaded: state.cached,
                        upload_enabled: state.upload_enabled,
                        location: state.location,
                    }
                })
                .collect();
            HistoryRow {
                account: entry.account_name,
                match_id: entry.match_id,
                timestamp: entry.record_start_timestamp.to_string(),
                map_name: entry.map_name,
                playlist: entry.playlist.to_string(),
                score: format!("{}-{}", entry.team0_score, entry.team1_score),
                upload_destinations,
            }
        })
        .collect())
}

#[cfg(not(target_arch = "wasm32"))]
async fn backfill_upload_destinations() -> Result<BackfillSummary, String> {
    use rlru::paths::AppPaths;
    use rlru::sync::{SyncOptions, SyncService};
    use rlru::Config;

    let paths = AppPaths::discover().map_err(|error| error.to_string())?;
    paths.ensure().map_err(|error| error.to_string())?;
    let config_path = paths.config_file();
    let config = Config::load_or_default(&config_path).map_err(|error| error.to_string())?;
    let summary = SyncService::new(paths, config)
        .run_once_with_options(SyncOptions {
            include_online: true,
            target_name: None,
            force: false,
            match_ids: Vec::new(),
        })
        .await
        .map_err(|error| error.to_string())?;

    Ok(BackfillSummary {
        uploaded: summary.uploaded,
        duplicates: summary.duplicates,
        cached: summary.cached,
        failed: summary.failed,
        failed_match_ids: summary.failed_match_ids,
        failed_uploads: summary
            .failed_uploads
            .into_iter()
            .map(|failed| ReplayUploadRequest {
                target_name: failed.target_name,
                match_id: failed.match_id,
                reason: Some(failed.reason),
            })
            .collect(),
    })
}

#[cfg(not(target_arch = "wasm32"))]
async fn upload_history_replay(request: ReplayUploadRequest) -> Result<BackfillSummary, String> {
    use rlru::paths::AppPaths;
    use rlru::sync::{SyncOptions, SyncService};
    use rlru::Config;

    let paths = AppPaths::discover().map_err(|error| error.to_string())?;
    paths.ensure().map_err(|error| error.to_string())?;
    let config_path = paths.config_file();
    let config = Config::load_or_default(&config_path).map_err(|error| error.to_string())?;
    let summary = SyncService::new(paths, config)
        .run_once_with_options(SyncOptions {
            include_online: true,
            target_name: Some(request.target_name),
            force: true,
            match_ids: vec![request.match_id],
        })
        .await
        .map_err(|error| error.to_string())?;

    if summary.matches_seen == 0 {
        return Err("No matching replay was found in current RL API history".to_string());
    }

    Ok(BackfillSummary {
        uploaded: summary.uploaded,
        duplicates: summary.duplicates,
        cached: summary.cached,
        failed: summary.failed,
        failed_match_ids: summary.failed_match_ids,
        failed_uploads: summary
            .failed_uploads
            .into_iter()
            .map(|failed| ReplayUploadRequest {
                target_name: failed.target_name,
                match_id: failed.match_id,
                reason: Some(failed.reason),
            })
            .collect(),
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn load_summary() -> AppSummary {
    use rlru::paths::AppPaths;
    use rlru::Config;

    match AppPaths::discover() {
        Ok(paths) => {
            let config_path = paths.config_file();
            let config = Config::load_or_default(&config_path).unwrap_or_default();
            let selected_account = config.behavior.selected_account.clone();
            let selected_upload_destination = config.behavior.selected_upload_destination.clone();
            AppSummary {
                config_path: config_path.display().to_string(),
                accounts: config
                    .accounts
                    .iter()
                    .map(|account| AccountSummary {
                        id: account.id,
                        name: account.name.clone(),
                        profile_id: account.profile_id,
                        platform: platform_label(&account.platform).to_string(),
                        unused: account.unused,
                        selected: selected_account.as_ref() == Some(&account.name),
                    })
                    .collect(),
                upload_destinations: config
                    .upload_destinations
                    .iter()
                    .map(|target| UploadDestinationSummary {
                        name: target.name.clone(),
                        url: target.url.to_string(),
                        primary: target.primary,
                        predefined: target.predefined,
                        upload_enabled: target.replay_upload.enabled,
                        auth: auth_label(&target.auth),
                        selected: selected_upload_destination.as_ref() == Some(&target.name),
                    })
                    .collect(),
                auto_upload: config.behavior.auto_upload,
                upload_on_launch: config.behavior.upload_on_launch,
                no_upload_while_connected: config.behavior.no_upload_while_connected,
                selected_account,
                selected_upload_destination,
                auto_upload_interval_minutes: config.behavior.auto_upload_interval.as_secs() / 60,
                auto_upload_jitter_minutes: config.behavior.auto_upload_jitter_max.as_secs() / 60,
                interval: format!(
                    "Every {} minutes",
                    config.behavior.auto_upload_interval.as_secs() / 60
                ),
                jitter: format!(
                    "{} minutes",
                    config.behavior.auto_upload_jitter_max.as_secs() / 60
                ),
                status: "Ready for auth, sync, and uploader runs".to_string(),
            }
        }
        Err(error) => AppSummary {
            config_path: error.to_string(),
            accounts: Vec::new(),
            upload_destinations: Vec::new(),
            auto_upload: false,
            upload_on_launch: false,
            no_upload_while_connected: false,
            selected_account: None,
            selected_upload_destination: None,
            auto_upload_interval_minutes: 45,
            auto_upload_jitter_minutes: 15,
            interval: "Unavailable".to_string(),
            jitter: "Unavailable".to_string(),
            status: "Could not discover local app paths".to_string(),
        },
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn platform_label(platform: &rlru::config::PlayerPlatform) -> &'static str {
    match platform {
        rlru::config::PlayerPlatform::Epic => "Epic",
        rlru::config::PlayerPlatform::Steam => "Steam",
        rlru::config::PlayerPlatform::PlayStation => "PlayStation",
        rlru::config::PlayerPlatform::Xbox => "Xbox",
        rlru::config::PlayerPlatform::Nintendo => "Nintendo",
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn auth_label(auth: &rlru::config::TargetAuth) -> String {
    match auth {
        rlru::config::TargetAuth::None => "No auth".to_string(),
        rlru::config::TargetAuth::AuthorizationHeader { .. } => "Authorization header".to_string(),
        rlru::config::TargetAuth::Bearer { .. } => "Bearer token".to_string(),
        rlru::config::TargetAuth::BearerEnv { variable } => {
            if std::env::var_os(variable).is_some() {
                format!("Bearer env token ({variable})")
            } else {
                format!("Bearer env token missing ({variable})")
            }
        }
        rlru::config::TargetAuth::BearerCommand { command } => command
            .first()
            .map(|program| format!("Bearer command token ({program})"))
            .unwrap_or_else(|| "Bearer command token missing command".to_string()),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn update_config(
    mut update: impl FnMut(&mut rlru::Config) -> Result<(), String>,
) -> Result<AppSummary, String> {
    use rlru::paths::AppPaths;
    use rlru::Config;

    let paths = AppPaths::discover().map_err(|error| error.to_string())?;
    paths.ensure().map_err(|error| error.to_string())?;
    let config_path = paths.config_file();
    let mut config = Config::load_or_default(&config_path).map_err(|error| error.to_string())?;
    update(&mut config)?;
    config
        .save(&config_path)
        .map_err(|error| error.to_string())?;
    Ok(load_summary())
}

#[cfg(not(target_arch = "wasm32"))]
fn update_behavior(
    mut update: impl FnMut(&mut rlru::config::BehaviorConfig),
) -> Result<AppSummary, String> {
    update_config(|config| {
        update(&mut config.behavior);
        Ok(())
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn add_account(input: AccountFormData) -> Result<AppSummary, String> {
    use rlru::config::AccountConfig;

    let name = input.name.trim();
    if name.is_empty() {
        return Err("Account name is required".to_string());
    }

    let profile_id = input
        .profile_id
        .parse::<u32>()
        .map_err(|_| "Profile must be a non-negative whole number".to_string())?;
    let platform = parse_platform(&input.platform)?;

    update_config(|config| {
        if config.accounts.iter().any(|account| account.name == name) {
            return Err(format!("Account {name:?} already exists"));
        }
        if config
            .accounts
            .iter()
            .any(|account| account.profile_id == profile_id)
        {
            return Err(format!(
                "Profile {profile_id} is already used by another account"
            ));
        }

        let next_id = config
            .accounts
            .iter()
            .map(|account| account.id)
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        config.accounts.push(AccountConfig {
            id: next_id,
            name: name.to_string(),
            profile_id,
            platform: platform.clone(),
            unused: input.unused,
        });
        Ok(())
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn remove_account(account_id: u32) -> Result<AppSummary, String> {
    update_config(|config| {
        if config.accounts.len() <= 1 {
            return Err("Config must keep at least one account".to_string());
        }

        let Some(index) = config
            .accounts
            .iter()
            .position(|account| account.id == account_id)
        else {
            return Err(format!("Account ID {account_id} no longer exists"));
        };

        let removed = config.accounts.remove(index);
        if config.behavior.selected_account.as_ref() == Some(&removed.name) {
            config.behavior.selected_account = None;
        }
        Ok(())
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn parse_platform(value: &str) -> Result<rlru::config::PlayerPlatform, String> {
    match value {
        "epic" => Ok(rlru::config::PlayerPlatform::Epic),
        "steam" => Ok(rlru::config::PlayerPlatform::Steam),
        "play_station" => Ok(rlru::config::PlayerPlatform::PlayStation),
        "xbox" => Ok(rlru::config::PlayerPlatform::Xbox),
        "nintendo" => Ok(rlru::config::PlayerPlatform::Nintendo),
        _ => Err(format!("Unsupported platform {value:?}")),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn save_auto_upload(enabled: bool) -> Result<AppSummary, String> {
    update_behavior(|behavior| behavior.auto_upload = enabled)
}

#[cfg(not(target_arch = "wasm32"))]
fn save_overview_config(input: OverviewConfigFormData) -> Result<AppSummary, String> {
    use std::time::Duration;

    let interval_minutes = parse_minutes(
        "sync interval",
        &input.auto_upload_interval_minutes,
        Some(1),
    )?;
    let jitter_minutes = parse_minutes("jitter max", &input.auto_upload_jitter_minutes, None)?;

    update_behavior(|behavior| {
        behavior.auto_upload_interval = Duration::from_secs(interval_minutes * 60);
        behavior.auto_upload_jitter_max = Duration::from_secs(jitter_minutes * 60);
        behavior.upload_on_launch = input.upload_on_launch;
        behavior.no_upload_while_connected = input.no_upload_while_connected;
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn parse_minutes(label: &str, value: &str, minimum: Option<u64>) -> Result<u64, String> {
    let minutes = value
        .trim()
        .parse::<u64>()
        .map_err(|_| format!("{label} must be a whole number of minutes"))?;

    if let Some(minimum) = minimum {
        if minutes < minimum {
            return Err(format!("{label} must be at least {minimum} minute"));
        }
    }

    Ok(minutes)
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
            profile_id: 1,
            platform: "Epic".to_string(),
            unused: false,
            selected: true,
        }],
        upload_destinations: vec![UploadDestinationSummary {
            name: "Rocket Sense".to_string(),
            url: "https://rocket-sense.duckdns.org/api/v1".to_string(),
            primary: true,
            predefined: true,
            upload_enabled: true,
            auth: "Bearer env token".to_string(),
            selected: true,
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

    let profile_id = input
        .profile_id
        .parse::<u32>()
        .map_err(|_| "Profile must be a non-negative whole number".to_string())?;
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
        profile_id,
        platform: platform_preview_label(&input.platform).to_string(),
        unused: input.unused,
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
