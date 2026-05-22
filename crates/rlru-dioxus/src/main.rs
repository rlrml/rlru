use dioxus::prelude::*;

const APP_CSS: &str = include_str!("../assets/styles.css");

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
    Storage,
    Activity,
}

impl ActiveView {
    const ALL: [Self; 5] = [
        Self::Overview,
        Self::History,
        Self::Accounts,
        Self::Storage,
        Self::Activity,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::History => "History",
            Self::Accounts => "Accounts",
            Self::Storage => "Storage",
            Self::Activity => "Activity",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::Overview => "Local auth, typed config, replay upload targets",
            Self::History => "Current RL API matches and Rocket Sense upload state",
            Self::Accounts => "Configured Rocket League account credentials",
            Self::Storage => "Upload destinations and replay storage state",
            Self::Activity => "Sync and uploader pipeline status",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct AppSummary {
    config_path: String,
    accounts: Vec<AccountSummary>,
    storage_targets: Vec<StorageSummary>,
    auto_upload: bool,
    exit_in_tray: bool,
    start_in_tray: bool,
    upload_on_launch: bool,
    no_upload_while_connected: bool,
    selected_account: Option<String>,
    selected_storage: Option<String>,
    interval: String,
    jitter: String,
    status: String,
}

impl AppSummary {
    fn account_count(&self) -> usize {
        self.accounts.len()
    }

    fn storage_count(&self) -> usize {
        self.storage_targets.len()
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

#[derive(Clone, Debug, PartialEq)]
struct StorageSummary {
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
    rocket_sense_state: String,
    rocket_sense_uploaded: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct BackfillSummary {
    uploaded: usize,
    duplicates: usize,
    cached: usize,
    failed: usize,
}

fn main() {
    launch_app();
}

#[cfg(feature = "desktop")]
fn launch_app() {
    use dioxus::desktop::{Config, WindowBuilder, WindowCloseBehaviour};

    let settings = load_desktop_settings();
    let close_behaviour = if settings.exit_in_tray {
        WindowCloseBehaviour::WindowHides
    } else {
        WindowCloseBehaviour::WindowCloses
    };

    dioxus::LaunchBuilder::desktop()
        .with_cfg(
            Config::new()
                .with_custom_head(desktop_head())
                .with_data_directory(desktop_data_dir())
                .with_background_color((243, 246, 244, 255))
                .with_close_behaviour(close_behaviour)
                .with_window(
                    WindowBuilder::new()
                        .with_title("rlru")
                        .with_visible(!settings.start_in_tray),
                ),
        )
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
    let active = active_view();
    let current_summary = summary();
    let message = action_message();
    let current_history = history.cloned();
    let history_status = history_message();
    let is_backfill_running = backfill_running();

    rsx! {
        document::Title { "rlru" }
        document::Meta {
            name: "viewport",
            content: "width=device-width, initial-scale=1, viewport-fit=cover",
        }
        document::Style { "{APP_CSS}" }
        DesktopTrayBridge { start_in_tray: current_summary.start_in_tray }
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
                        OverviewView { summary: current_summary }
                    },
                    ActiveView::History => rsx! {
                        HistoryView {
                            history: current_history,
                            message: history_status,
                            backfill_running: is_backfill_running,
                            onrefresh: move |_| {
                                history.restart();
                                history_message.set(String::new());
                            },
                            onbackfill: move |_| {
                                backfill_running.set(true);
                                history_message.set("Backfilling Rocket Sense from current RL API history".to_string());
                                spawn(async move {
                                    match backfill_rocket_sense().await {
                                        Ok(summary) => {
                                            history_message.set(format!(
                                                "Backfill complete: {} uploaded, {} duplicates, {} cached, {} failed",
                                                summary.uploaded,
                                                summary.duplicates,
                                                summary.cached,
                                                summary.failed
                                            ));
                                            history.restart();
                                        }
                                        Err(error) => history_message.set(error),
                                    }
                                    backfill_running.set(false);
                                });
                            },
                        }
                    },
                    ActiveView::Accounts => rsx! {
                        AccountsView { summary: current_summary }
                    },
                    ActiveView::Storage => rsx! {
                        StorageView { summary: current_summary }
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
fn OverviewView(summary: AppSummary) -> Element {
    let auto_upload_value = if summary.auto_upload {
        "Enabled"
    } else {
        "Disabled"
    }
    .to_string();

    rsx! {
        div { class: "summary-grid",
            Metric { label: "Accounts", value: summary.account_count().to_string() }
            Metric { label: "Upload Targets", value: summary.storage_count().to_string() }
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
                dt { "Storage" }
                dd { "{summary.selected_storage.clone().unwrap_or_else(|| \"All configured targets\".to_string())}" }
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
    onrefresh: EventHandler<()>,
    onbackfill: EventHandler<()>,
) -> Element {
    let backfill_label = if backfill_running {
        "Backfilling..."
    } else {
        "Backfill Rocket Sense"
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
                                span { "Rocket Sense" }
                            }
                            for row in rows {
                                div { class: "history-row",
                                    span { class: "mono-cell", "{short_match_id(&row.match_id)}" }
                                    span { "{row.account}" }
                                    span { "{row.timestamp}" }
                                    span { "{row.map_name} / {row.playlist}" }
                                    span { "{row.score}" }
                                    span {
                                        class: if row.rocket_sense_uploaded { "state-pill uploaded" } else { "state-pill missing" },
                                        "{row.rocket_sense_state}"
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
fn AccountsView(summary: AppSummary) -> Element {
    rsx! {
        section { class: "panel compact-panel",
            div { class: "panel-header",
                h2 { "Accounts" }
                span { "{summary.account_count()} configured" }
            }
            div { class: "account-list",
                for account in summary.accounts {
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
                    }
                }
            }
        }
    }
}

#[component]
fn StorageView(summary: AppSummary) -> Element {
    rsx! {
        section { class: "panel compact-panel",
            div { class: "panel-header",
                h2 { "Storage" }
                span { "{summary.storage_count()} targets" }
            }
            div { class: "account-list",
                for target in summary.storage_targets {
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
fn DesktopTrayBridge(start_in_tray: bool) -> Element {
    use dioxus::desktop::default_icon;
    use dioxus::desktop::trayicon::menu::{Menu, MenuItem, PredefinedMenuItem};
    use dioxus::desktop::trayicon::TrayIconBuilder;
    use dioxus::desktop::use_tray_menu_event_handler;
    use dioxus::desktop::{window, WindowCloseBehaviour};

    use_hook(move || {
        let menu = Menu::new();
        let show = MenuItem::with_id("rlru-show-window", "Open rlru", true, None);
        let hide = MenuItem::with_id("rlru-hide-window", "Hide Window", true, None);
        let quit = MenuItem::with_id("rlru-quit", "Quit", true, None);
        let separator = PredefinedMenuItem::separator();
        if let Err(error) = menu.append_items(&[&show, &hide, &separator, &quit]) {
            eprintln!("Failed to build rlru tray menu: {error}");
        }

        let mut builder = TrayIconBuilder::new()
            .with_id("rlru")
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .with_title("rlru")
            .with_tooltip("rlru");

        match default_icon() {
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
    });

    use_tray_menu_event_handler(move |event| match event.id().as_ref() {
        "rlru-show-window" => show_window(),
        "rlru-hide-window" => window().set_visible(false),
        "rlru-quit" => {
            let win = window();
            win.set_close_behavior(WindowCloseBehaviour::WindowCloses);
            win.close();
        }
        _ => {}
    });

    rsx! {}
}

#[cfg(not(all(
    feature = "desktop",
    not(any(target_os = "ios", target_os = "android"))
)))]
#[component]
fn DesktopTrayBridge(start_in_tray: bool) -> Element {
    let _ = start_in_tray;
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
        .current_history(Some("Rocket Sense"))
        .await
        .map_err(|error| error.to_string())?;

    Ok(entries
        .into_iter()
        .map(|entry| {
            let rocket_sense = entry
                .upload_states
                .iter()
                .find(|state| state.target_name == "Rocket Sense");
            let (rocket_sense_state, rocket_sense_uploaded) = match rocket_sense {
                Some(state) if state.cached => ("Uploaded".to_string(), true),
                Some(state) if !state.upload_enabled => ("Disabled".to_string(), false),
                Some(_) => ("Not uploaded".to_string(), false),
                None => ("Not configured".to_string(), false),
            };
            HistoryRow {
                account: entry.account_name,
                match_id: entry.match_id,
                timestamp: entry.record_start_timestamp.to_string(),
                map_name: entry.map_name,
                playlist: entry.playlist.to_string(),
                score: format!("{}-{}", entry.team0_score, entry.team1_score),
                rocket_sense_state,
                rocket_sense_uploaded,
            }
        })
        .collect())
}

#[cfg(not(target_arch = "wasm32"))]
async fn backfill_rocket_sense() -> Result<BackfillSummary, String> {
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
            target_name: Some("Rocket Sense".to_string()),
            force: false,
        })
        .await
        .map_err(|error| error.to_string())?;

    Ok(BackfillSummary {
        uploaded: summary.uploaded,
        duplicates: summary.duplicates,
        cached: summary.cached,
        failed: summary.failed,
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
            let selected_storage = config.behavior.selected_storage.clone();
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
                storage_targets: config
                    .storage
                    .iter()
                    .map(|target| StorageSummary {
                        name: target.name.clone(),
                        url: target.url.to_string(),
                        primary: target.primary,
                        predefined: target.predefined,
                        upload_enabled: target.replay_upload.enabled,
                        auth: auth_label(&target.auth).to_string(),
                        selected: selected_storage.as_ref() == Some(&target.name),
                    })
                    .collect(),
                auto_upload: config.behavior.auto_upload,
                exit_in_tray: config.behavior.exit_in_tray,
                start_in_tray: config.behavior.start_in_tray,
                upload_on_launch: config.behavior.upload_on_launch,
                no_upload_while_connected: config.behavior.no_upload_while_connected,
                selected_account,
                selected_storage,
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
            storage_targets: Vec::new(),
            auto_upload: false,
            exit_in_tray: true,
            start_in_tray: true,
            upload_on_launch: false,
            no_upload_while_connected: false,
            selected_account: None,
            selected_storage: None,
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
fn update_behavior(
    mut update: impl FnMut(&mut rlru::config::BehaviorConfig),
) -> Result<AppSummary, String> {
    use rlru::paths::AppPaths;
    use rlru::Config;

    let paths = AppPaths::discover().map_err(|error| error.to_string())?;
    paths.ensure().map_err(|error| error.to_string())?;
    let config_path = paths.config_file();
    let mut config = Config::load_or_default(&config_path).map_err(|error| error.to_string())?;
    update(&mut config.behavior);
    config
        .save(&config_path)
        .map_err(|error| error.to_string())?;
    Ok(load_summary())
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
            rocket_sense_state: "Uploaded".to_string(),
            rocket_sense_uploaded: true,
        },
        HistoryRow {
            account: "colonelpanic8".to_string(),
            match_id: "F90812E5EFDA4CC4AC7903596F02E6AB".to_string(),
            timestamp: "1699999000".to_string(),
            map_name: "Mannfield".to_string(),
            playlist: "13".to_string(),
            score: "1-4".to_string(),
            rocket_sense_state: "Not uploaded".to_string(),
            rocket_sense_uploaded: false,
        },
    ])
}

#[cfg(target_arch = "wasm32")]
async fn backfill_rocket_sense() -> Result<BackfillSummary, String> {
    Ok(BackfillSummary {
        uploaded: 1,
        duplicates: 0,
        cached: 1,
        failed: 0,
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
        storage_targets: vec![StorageSummary {
            name: "Rocket Sense".to_string(),
            url: "http://127.0.0.1:8080/api/v1".to_string(),
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
        selected_storage: Some("Rocket Sense".to_string()),
        interval: "Every 45 minutes".to_string(),
        jitter: "15 minutes".to_string(),
        status: "Ready for auth, sync, and uploader runs".to_string(),
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
