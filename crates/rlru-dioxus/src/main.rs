use dioxus::prelude::*;

const APP_CSS: &str = include_str!("../assets/styles.css");
#[cfg(feature = "desktop")]
const APP_ICON_PNG: &[u8] = include_bytes!("../assets/icons/rlru-icon-1024.png");

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

#[cfg(feature = "desktop")]
#[derive(Clone, Copy, Debug)]
struct DesktopSettings {
    exit_in_tray: bool,
    start_in_tray: bool,
}

#[cfg(feature = "desktop")]
impl Default for DesktopSettings {
    fn default() -> Self {
        Self {
            exit_in_tray: true,
            start_in_tray: true,
        }
    }
}

#[cfg(feature = "desktop")]
fn load_desktop_settings() -> DesktopSettings {
    use rlru::paths::AppPaths;
    use rlru::Config;

    AppPaths::discover()
        .ok()
        .and_then(|paths| {
            Config::load_or_default(&paths.config_file())
                .ok()
                .map(|config| DesktopSettings {
                    exit_in_tray: config.behavior.exit_in_tray,
                    start_in_tray: config.behavior.start_in_tray,
                })
        })
        .unwrap_or_default()
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
    exit_in_tray: bool,
    start_in_tray: bool,
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

    let settings = load_desktop_settings();
    let close_behaviour = if settings.exit_in_tray {
        WindowCloseBehaviour::WindowHides
    } else {
        WindowCloseBehaviour::WindowCloses
    };
    let mut config = Config::new()
        .with_custom_head(desktop_head())
        .with_data_directory(desktop_data_dir())
        .with_background_color((243, 246, 244, 255))
        .with_close_behaviour(close_behaviour)
        .with_window(
            WindowBuilder::new()
                .with_title("rlru")
                .with_visible(!settings.start_in_tray),
        );

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
    let mut history = use_resource(load_history);
    let mut active_view = use_signal(|| ActiveView::Overview);
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
            start_in_tray: current_summary.start_in_tray,
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
                                &run_summary.failed_match_ids,
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
                            failures.retain(|failure| failure != &request);
                            for failure in &run_summary.failed_uploads {
                                if !failures.contains(failure) {
                                    failures.push(failure.clone());
                                }
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
                                &run_summary.failed_match_ids,
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
        DesktopWindowBehavior { exit_in_tray: current_summary.exit_in_tray }
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
                                                &run_summary.failed_match_ids,
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
                                            failures.retain(|failure| failure != &request);
                                            for failure in &run_summary.failed_uploads {
                                                if !failures.contains(failure) {
                                                    failures.push(failure.clone());
                                                }
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
                                                &run_summary.failed_match_ids,
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
                            onstartintray: move |enabled| {
                                match save_start_in_tray(enabled) {
                                    Ok(updated) => {
                                        summary.set(updated);
                                        action_message.set(if enabled {
                                            "Startup now hides the window to the tray".to_string()
                                        } else {
                                            "Startup now opens the window".to_string()
                                        });
                                    }
                                    Err(error) => action_message.set(error),
                                }
                            },
                            onexitintray: move |enabled| {
                                match save_exit_in_tray(enabled) {
                                    Ok(updated) => {
                                        summary.set(updated);
                                        action_message.set(if enabled {
                                            "Closing the window now hides to tray".to_string()
                                        } else {
                                            "Closing the window now exits the app".to_string()
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
                                                            disabled: uploading.is_some(),
                                                            onclick: {
                                                                let target_name = destination.target_name.clone();
                                                                let match_id = row.match_id.clone();
                                                                move |_| {
                                                                    onupload.call(ReplayUploadRequest {
                                                                        target_name: target_name.clone(),
                                                                        match_id: match_id.clone(),
                                                                    });
                                                                }
                                                            },
                                                            if uploading.as_ref().is_some_and(|request| request.target_name == destination.target_name && request.match_id == row.match_id) {
                                                                "Linking"
                                                            } else {
                                                                "Link"
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
                                                        disabled: uploading.is_some(),
                                                        onclick: {
                                                            let target_name = destination.target_name.clone();
                                                            let match_id = row.match_id.clone();
                                                            move |_| {
                                                                onupload.call(ReplayUploadRequest {
                                                                    target_name: target_name.clone(),
                                                                    match_id: match_id.clone(),
                                                                });
                                                            }
                                                        },
                                                        if uploading.as_ref().is_some_and(|request| request.target_name == destination.target_name && request.match_id == row.match_id) {
                                                            "Uploading"
                                                        } else if is_failed_upload(&failed_uploads, &destination.target_name, &row.match_id) {
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
fn ActivityView(
    summary: AppSummary,
    onautoupload: EventHandler<bool>,
    onstartintray: EventHandler<bool>,
    onexitintray: EventHandler<bool>,
) -> Element {
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
    let next_start_in_tray = !summary.start_in_tray;
    let start_in_tray_label = if summary.start_in_tray {
        "Hidden in tray"
    } else {
        "Window opens"
    };
    let start_in_tray_action = if summary.start_in_tray {
        "Open window at startup"
    } else {
        "Start hidden in tray"
    };
    let next_exit_in_tray = !summary.exit_in_tray;
    let exit_in_tray_label = if summary.exit_in_tray {
        "Close hides window"
    } else {
        "Close exits app"
    };
    let exit_in_tray_action = if summary.exit_in_tray {
        "Exit on close"
    } else {
        "Hide on close"
    };
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
                div { class: if summary.start_in_tray { "status-dot" } else { "status-dot off" } }
                p { "Startup: {start_in_tray_label}" }
                button {
                    class: "secondary-button",
                    onclick: move |_| onstartintray.call(next_start_in_tray),
                    "{start_in_tray_action}"
                }
            }
            div { class: "activity-row main-action",
                div { class: if summary.exit_in_tray { "status-dot" } else { "status-dot off" } }
                p { "Window close: {exit_in_tray_label}" }
                button {
                    class: "secondary-button",
                    onclick: move |_| onexitintray.call(next_exit_in_tray),
                    "{exit_in_tray_action}"
                }
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
#[component]
fn DesktopTrayBridge(
    start_in_tray: bool,
    summary: AppSummary,
    history: Option<Result<Vec<HistoryRow>, String>>,
    sync_run: SyncRunState,
    failed_uploads: Vec<ReplayUploadRequest>,
    onsync: EventHandler<()>,
    onrefreshhistory: EventHandler<()>,
    onretry: EventHandler<ReplayUploadRequest>,
) -> Element {
    use dioxus::desktop::icon_from_memory;
    use dioxus::desktop::trayicon::{DioxusTrayIcon, TrayIconBuilder};
    use dioxus::desktop::use_tray_menu_event_handler;
    use dioxus::desktop::{window, WindowCloseBehaviour};

    let tray_icon = use_hook({
        let summary = summary.clone();
        let history = history.clone();
        let sync_run = sync_run.clone();
        let failed_uploads = failed_uploads.clone();
        move || {
            let menu = build_tray_menu(&summary, history.as_ref(), &sync_run, &failed_uploads);
            let mut builder = TrayIconBuilder::new()
                .with_id("rlru")
                .with_menu(Box::new(menu))
                .with_menu_on_left_click(false)
                .with_title("rlru")
                .with_tooltip(tray_tooltip(&summary, &sync_run, failed_uploads.len()));

            match icon_from_memory::<DioxusTrayIcon>(APP_ICON_PNG) {
                Ok(icon) => builder = builder.with_icon(icon),
                Err(error) => eprintln!("Failed to load rlru tray icon: {error}"),
            }

            match builder.build() {
                Ok(tray) => Some(tray),
                Err(error) => {
                    eprintln!("Failed to initialize rlru tray icon: {error}");
                    if start_in_tray {
                        window().set_visible(true);
                        window().set_focus();
                    }
                    None
                }
            }
        }
    });

    let failed_uploads_for_effect = failed_uploads.clone();
    use_effect(use_reactive!(|(
        summary,
        history,
        sync_run,
        failed_uploads_for_effect,
    )| {
        if let Some(tray) = tray_icon.as_ref() {
            let tooltip = tray_tooltip(&summary, &sync_run, failed_uploads_for_effect.len());
            if let Err(error) = tray.set_tooltip(Some(tooltip)) {
                eprintln!("Failed to update rlru tray tooltip: {error}");
            }
            tray.set_menu(Some(Box::new(build_tray_menu(
                &summary,
                history.as_ref(),
                &sync_run,
                &failed_uploads_for_effect,
            ))));
        }
    }));

    use_tray_menu_event_handler(move |event| match event.id().as_ref() {
        "rlru-show-window" => show_window(),
        "rlru-hide-window" => window().set_visible(false),
        "rlru-sync-now" => onsync.call(()),
        "rlru-refresh-history" => onrefreshhistory.call(()),
        "rlru-quit" => {
            let win = window();
            win.set_close_behavior(WindowCloseBehaviour::WindowCloses);
            win.close();
        }
        id => {
            if let Some(request) = failed_uploads
                .iter()
                .find(|request| tray_retry_menu_id(request) == id)
            {
                onretry.call(request.clone());
            }
        }
    });

    rsx! {}
}

#[cfg(all(
    feature = "desktop",
    not(any(target_os = "ios", target_os = "android"))
))]
fn build_tray_menu(
    summary: &AppSummary,
    history: Option<&Result<Vec<HistoryRow>, String>>,
    sync_run: &SyncRunState,
    failed_uploads: &[ReplayUploadRequest],
) -> dioxus::desktop::trayicon::menu::Menu {
    use dioxus::desktop::trayicon::menu::{
        IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu,
    };

    let menu = Menu::new();
    let status = MenuItem::new(tray_sync_label(sync_run), false, None);
    let cadence = MenuItem::new(
        format!(
            "Auto upload: {}, {}",
            auto_upload_label(summary),
            summary.interval
        ),
        false,
        None,
    );
    let sync_now = MenuItem::with_id("rlru-sync-now", "Sync Now", !sync_run.running, None);
    let refresh_history = MenuItem::with_id("rlru-refresh-history", "Refresh History", true, None);
    let separator = PredefinedMenuItem::separator();
    if let Err(error) = menu.append_items(&[
        &status as &dyn IsMenuItem,
        &cadence,
        &sync_now,
        &refresh_history,
        &separator,
    ]) {
        eprintln!("Failed to build rlru tray menu: {error}");
    }

    let history_menu = Submenu::new("History", true);
    append_history_menu(&history_menu, history, failed_uploads);
    if let Err(error) = menu.append(&history_menu) {
        eprintln!("Failed to append rlru tray history menu: {error}");
    }

    if !failed_uploads.is_empty() {
        let separator = PredefinedMenuItem::separator();
        let failures_menu =
            Submenu::new(format!("Failed Uploads ({})", failed_uploads.len()), true);
        for request in failed_uploads {
            let retry = MenuItem::with_id(
                tray_retry_menu_id(request),
                format!(
                    "Retry {} to {}",
                    short_match_id(&request.match_id),
                    request.target_name
                ),
                true,
                None,
            );
            if let Err(error) = failures_menu.append(&retry) {
                eprintln!("Failed to append rlru failed upload menu item: {error}");
            }
        }
        if let Err(error) = menu.append_items(&[&separator, &failures_menu]) {
            eprintln!("Failed to append rlru failed upload menu: {error}");
        }
    }

    let separator = PredefinedMenuItem::separator();
    let show = MenuItem::with_id("rlru-show-window", "Open rlru", true, None);
    let hide = MenuItem::with_id("rlru-hide-window", "Hide Window", true, None);
    let quit = MenuItem::with_id("rlru-quit", "Quit", true, None);
    if let Err(error) = menu.append_items(&[&separator, &show, &hide, &quit]) {
        eprintln!("Failed to append rlru tray window menu: {error}");
    }

    menu
}

#[cfg(all(
    feature = "desktop",
    not(any(target_os = "ios", target_os = "android"))
))]
fn append_history_menu(
    menu: &dioxus::desktop::trayicon::menu::Submenu,
    history: Option<&Result<Vec<HistoryRow>, String>>,
    failed_uploads: &[ReplayUploadRequest],
) {
    use dioxus::desktop::trayicon::menu::MenuItem;

    match history {
        None => {
            let item = MenuItem::new("Loading current history", false, None);
            let _ = menu.append(&item);
        }
        Some(Err(error)) => {
            let item = MenuItem::new(format!("History unavailable: {error}"), false, None);
            let _ = menu.append(&item);
        }
        Some(Ok(rows)) if rows.is_empty() => {
            let item = MenuItem::new("No current history entries", false, None);
            let _ = menu.append(&item);
        }
        Some(Ok(rows)) => {
            for row in rows.iter().take(8) {
                let row_menu = dioxus::desktop::trayicon::menu::Submenu::new(
                    format!(
                        "{} - {} - {}",
                        short_match_id(&row.match_id),
                        row.map_name,
                        row.score
                    ),
                    true,
                );
                let account = MenuItem::new(format!("Account: {}", row.account), false, None);
                let when = MenuItem::new(format!("When: {}", row.timestamp), false, None);
                let _ = row_menu.append(&account);
                let _ = row_menu.append(&when);
                for destination in &row.upload_destinations {
                    let state = if is_failed_upload(
                        failed_uploads,
                        &destination.target_name,
                        &row.match_id,
                    ) {
                        "Failed"
                    } else {
                        destination.state.as_str()
                    };
                    let item = MenuItem::new(
                        format!("{}: {}", destination.target_name, state),
                        false,
                        None,
                    );
                    let _ = row_menu.append(&item);
                }
                let _ = menu.append(&row_menu);
            }
        }
    }
}

#[cfg(not(all(
    feature = "desktop",
    not(any(target_os = "ios", target_os = "android"))
)))]
#[component]
fn DesktopTrayBridge(
    start_in_tray: bool,
    summary: AppSummary,
    history: Option<Result<Vec<HistoryRow>, String>>,
    sync_run: SyncRunState,
    failed_uploads: Vec<ReplayUploadRequest>,
    onsync: EventHandler<()>,
    onrefreshhistory: EventHandler<()>,
    onretry: EventHandler<ReplayUploadRequest>,
) -> Element {
    let _ = (
        start_in_tray,
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

#[cfg(feature = "desktop")]
#[component]
fn DesktopWindowBehavior(exit_in_tray: bool) -> Element {
    use dioxus::desktop::{window, WindowCloseBehaviour};

    use_effect(use_reactive!(|exit_in_tray| {
        let behaviour = if exit_in_tray {
            WindowCloseBehaviour::WindowHides
        } else {
            WindowCloseBehaviour::WindowCloses
        };
        window().set_close_behavior(behaviour);
    }));

    rsx! {}
}

#[cfg(not(feature = "desktop"))]
#[component]
fn DesktopWindowBehavior(exit_in_tray: bool) -> Element {
    let _ = exit_in_tray;
    rsx! {}
}

#[cfg(feature = "desktop")]
fn show_window() {
    let win = dioxus::desktop::window();
    win.set_visible(true);
    win.set_focus();
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

fn format_backfill_message(message: String, failed_match_ids: &[String]) -> String {
    if failed_match_ids.is_empty() {
        message
    } else {
        format!(
            "{message}; failed matches: {}",
            failed_match_ids
                .iter()
                .map(|match_id| short_match_id(match_id))
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

fn dedupe_upload_requests(requests: Vec<ReplayUploadRequest>) -> Vec<ReplayUploadRequest> {
    let mut deduped = Vec::new();
    for request in requests {
        if !deduped.contains(&request) {
            deduped.push(request);
        }
    }
    deduped
}

fn is_failed_upload(
    failed_uploads: &[ReplayUploadRequest],
    target_name: &str,
    match_id: &str,
) -> bool {
    failed_uploads
        .iter()
        .any(|failure| failure.target_name == target_name && failure.match_id == match_id)
}

#[cfg(all(
    feature = "desktop",
    not(any(target_os = "ios", target_os = "android"))
))]
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

#[cfg(all(
    feature = "desktop",
    not(any(target_os = "ios", target_os = "android"))
))]
#[cfg(all(
    feature = "desktop",
    not(any(target_os = "ios", target_os = "android"))
))]
fn tray_retry_menu_id(request: &ReplayUploadRequest) -> String {
    format!("rlru-retry:{}:{}", request.target_name, request.match_id)
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
                        auth: auth_label(&target.auth).to_string(),
                        selected: selected_upload_destination.as_ref() == Some(&target.name),
                    })
                    .collect(),
                auto_upload: config.behavior.auto_upload,
                exit_in_tray: config.behavior.exit_in_tray,
                start_in_tray: config.behavior.start_in_tray,
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
            exit_in_tray: true,
            start_in_tray: true,
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
fn auth_label(auth: &rlru::config::TargetAuth) -> &'static str {
    match auth {
        rlru::config::TargetAuth::None => "No auth",
        rlru::config::TargetAuth::AuthorizationHeader { .. } => "Authorization header",
        rlru::config::TargetAuth::Bearer { .. } => "Bearer token",
        rlru::config::TargetAuth::BearerEnv { .. } => "Bearer env token",
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
fn save_start_in_tray(enabled: bool) -> Result<AppSummary, String> {
    update_behavior(|behavior| behavior.start_in_tray = enabled)
}

#[cfg(not(target_arch = "wasm32"))]
fn save_exit_in_tray(enabled: bool) -> Result<AppSummary, String> {
    update_behavior(|behavior| behavior.exit_in_tray = enabled)
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
        exit_in_tray: true,
        start_in_tray: true,
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
fn save_start_in_tray(enabled: bool) -> Result<AppSummary, String> {
    let mut summary = load_summary();
    summary.start_in_tray = enabled;
    Ok(summary)
}

#[cfg(target_arch = "wasm32")]
fn save_exit_in_tray(enabled: bool) -> Result<AppSummary, String> {
    let mut summary = load_summary();
    summary.exit_in_tray = enabled;
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
