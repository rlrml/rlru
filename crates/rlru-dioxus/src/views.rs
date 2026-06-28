use dioxus::prelude::*;

use crate::model::*;
use crate::ActiveView;

const ROCKET_SENSE_LOGO: &str = include_str!("../assets/icons/rocket-sense-logo.svg");
#[component]
pub(crate) fn Sidebar(
    active: ActiveView,
    open: bool,
    onselect: EventHandler<ActiveView>,
    ontoggle: EventHandler<()>,
) -> Element {
    let class = if open { "app-nav open" } else { "app-nav" };

    rsx! {
        aside {
            class: "{class}",
            div { class: "nav-header",
                div { class: "nav-title",
                    span {
                        class: "nav-brand-row",
                        span {
                            class: "rocket-sense-logo",
                            aria_hidden: "true",
                            dangerous_inner_html: ROCKET_SENSE_LOGO,
                        }
                        strong { "rlru" }
                    }
                }
                button {
                    class: "mobile-nav-toggle",
                    r#type: "button",
                    aria_label: "Toggle navigation",
                    aria_expanded: "{open}",
                    onclick: move |_| ontoggle.call(()),
                    span { aria_hidden: "true" }
                    span { aria_hidden: "true" }
                    span { aria_hidden: "true" }
                }
            }
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
pub(crate) fn NavButton(
    view: ActiveView,
    selected: bool,
    onclick: EventHandler<MouseEvent>,
) -> Element {
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
pub(crate) fn OverviewView(
    summary: AppSummary,
    onsave: EventHandler<OverviewConfigFormData>,
) -> Element {
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
    let mut window_decorations = use_signal(|| summary.window_decorations.clone());

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
                label {
                    span { "Window decorations" }
                    select {
                        value: "{window_decorations}",
                        onchange: move |event| window_decorations.set(event.value()),
                        option { value: "auto", "Auto" }
                        option { value: "system", "System" }
                        option { value: "hidden", "Hidden" }
                    }
                }
                button {
                    class: "primary-button form-submit",
                    onclick: move |_| {
                        onsave.call(OverviewConfigFormData {
                            auto_upload_interval_minutes: interval_minutes().trim().to_string(),
                            auto_upload_jitter_minutes: jitter_minutes().trim().to_string(),
                            upload_on_launch: upload_on_launch(),
                            no_upload_while_connected: no_upload_while_connected(),
                            window_decorations: window_decorations(),
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
pub(crate) fn HistoryView(
    history: Option<Result<Vec<HistoryRow>, String>>,
    message: String,
    backfill_running: bool,
    active_uploads: Vec<ActiveUpload>,
    queue_completed: usize,
    queue_total: usize,
    failed_uploads: Vec<ReplayUploadRequest>,
    onrefresh: EventHandler<()>,
    onbackfill: EventHandler<Vec<ReplayUploadRequest>>,
    onupload: EventHandler<ReplayUploadRequest>,
) -> Element {
    let backfill_requests = history
        .as_ref()
        .and_then(|history| history.as_ref().ok())
        .map(|rows| history_upload_requests(rows, &active_uploads))
        .unwrap_or_default();
    let backfill_count = backfill_requests.len();
    let backfill_label = if backfill_running {
        "Backfilling..."
    } else if backfill_count == 1 {
        "Queue 1 Upload"
    } else if backfill_count > 1 {
        "Queue Backfill"
    } else {
        "Backfill Destinations"
    };
    let backfill_disabled = backfill_running || backfill_count == 0;

    rsx! {
        section { class: "panel history-panel",
            div { class: "panel-header",
                h2 { "RL API History" }
                div { class: "button-row",
                    button {
                        class: "secondary-button",
                        onclick: move |_| onrefresh.call(()),
                        "Refresh History"
                    }
                    button {
                        class: "primary-button",
                        disabled: backfill_disabled,
                        onclick: move |_| {
                            if !backfill_disabled {
                                onbackfill.call(backfill_requests.clone());
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
                Some(Ok(rows)) => {
                    let upload_activity = history_upload_activity(
                        &rows,
                        &active_uploads,
                        &failed_uploads,
                        backfill_running,
                        queue_completed,
                        queue_total,
                    );
                    rsx! {
                    if rows.is_empty() {
                        p { class: "empty-state", "No current RL API history entries found." }
                    } else {
                        UploadActivitySummary { activity: upload_activity }
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
                                                match history_upload_control(
                                                    &destination,
                                                    &row.match_id,
                                                    &active_uploads,
                                                    &failed_uploads,
                                                    backfill_running,
                                                ) {
                                                    HistoryUploadControl::OpenLink { location } => rsx! {
                                                        a {
                                                            class: "state-pill uploaded",
                                                            href: "{location}",
                                                            target: "_blank",
                                                            "Open"
                                                        }
                                                    },
                                                    HistoryUploadControl::Label { class_name, label } => rsx! {
                                                        span {
                                                            class: "{class_name}",
                                                            "{label}"
                                                        }
                                                    },
                                                    HistoryUploadControl::Button {
                                                        class_name,
                                                        label,
                                                        title,
                                                        disabled,
                                                        request,
                                                    } => rsx! {
                                                        button {
                                                            class: "{class_name}",
                                                            title: "{title}",
                                                            disabled,
                                                            onclick: move |_| {
                                                                if !disabled {
                                                                    onupload.call(request.clone());
                                                                }
                                                            },
                                                            "{label}"
                                                        }
                                                    },
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
pub(crate) fn UploadActivitySummary(activity: HistoryUploadActivity) -> Element {
    let progress = activity.progress.clone();
    rsx! {
        div { class: "{activity.class_name}",
            div { class: "upload-activity-copy",
                strong { "{activity.headline}" }
                span { "{activity.detail}" }
            }
            if let Some(progress) = progress {
                div { class: "upload-progress",
                    div {
                        class: "upload-progress-bar",
                        aria_label: "Upload progress",
                        div {
                            class: "upload-progress-fill",
                            style: "width: {progress.percent}%;",
                        }
                    }
                    span { "{progress.completed}/{progress.total}" }
                }
            }
            div { class: "upload-activity-metrics",
                for metric in activity.metrics {
                    span { class: "upload-activity-metric",
                        small { "{metric.label}" }
                        b { "{metric.value}" }
                    }
                }
            }
        }
    }
}

#[component]
pub(crate) fn AccountsView(
    summary: AppSummary,
    auth_prompt: Option<AccountAuthPrompt>,
    auth_running: bool,
    onadd: EventHandler<AccountFormData>,
    onauth: EventHandler<u32>,
    onregenauth: EventHandler<u32>,
    onfinishauth: EventHandler<(AccountAuthPrompt, String)>,
    oncancelauth: EventHandler<()>,
    onremove: EventHandler<u32>,
) -> Element {
    let accounts = summary.accounts.clone();
    let account_count = accounts.len();
    let mut account_name = use_signal(String::new);
    let mut platform = use_signal(|| "epic".to_string());
    let mut sync_enabled = use_signal(|| true);
    let mut authenticate = use_signal(|| true);
    let mut auth_code = use_signal(String::new);
    let mut confirming_remove = use_signal(|| None::<u32>);
    let auth_prompt_for_submit = auth_prompt.clone();
    let auth_prompt_account_id = auth_prompt.as_ref().map(|prompt| prompt.account_id);
    let can_remove = account_count > 1;
    let submit_label = if authenticate() && platform() == "epic" {
        "Add & Authenticate"
    } else {
        "Add Account"
    };

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
                        oninput: move |event| {
                            let selected = event.value();
                            if selected != "epic" {
                                authenticate.set(false);
                            }
                            platform.set(selected);
                        },
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
                label { class: "checkbox-field",
                    input {
                        r#type: "checkbox",
                        checked: authenticate(),
                        disabled: platform() != "epic",
                        oninput: move |event| authenticate.set(event.checked()),
                    }
                    span { "Epic Auth" }
                }
                button {
                    class: "primary-button form-submit",
                    disabled: auth_running,
                    onclick: move |_| {
                        onadd.call(AccountFormData {
                            name: account_name().trim().to_string(),
                            platform: platform(),
                            sync_enabled: sync_enabled(),
                            authenticate: authenticate() && platform() == "epic",
                        });
                        account_name.set(String::new());
                        platform.set("epic".to_string());
                        sync_enabled.set(true);
                        authenticate.set(true);
                    },
                    "{submit_label}"
                }
            }
            if let Some(prompt) = auth_prompt {
                div { class: "auth-prompt",
                    div {
                        strong { "{prompt.account_name}" }
                        span { "Epic authorization code" }
                    }
                    input {
                        value: "{auth_code}",
                        placeholder: "Authorization code",
                        disabled: auth_running,
                        oninput: move |event| auth_code.set(event.value()),
                    }
                    div { class: "auth-actions",
                        a {
                            class: "secondary-button",
                            href: "{prompt.login_url}",
                            target: "_blank",
                            rel: "noreferrer",
                            "Open Epic"
                        }
                        button {
                            class: "primary-button",
                            disabled: auth_running || auth_code().trim().is_empty(),
                            onclick: move |_| {
                                if let Some(prompt) = auth_prompt_for_submit.clone() {
                                    onfinishauth.call((prompt, auth_code()));
                                    auth_code.set(String::new());
                                }
                            },
                            "Save Login"
                        }
                        button {
                            class: "secondary-button",
                            disabled: auth_running,
                            onclick: move |_| {
                                auth_code.set(String::new());
                                if let Some(account_id) = auth_prompt_account_id {
                                    onregenauth.call(account_id);
                                }
                            },
                            "New Link"
                        }
                        button {
                            class: "secondary-button",
                            disabled: auth_running,
                            onclick: move |_| oncancelauth.call(()),
                            "Cancel"
                        }
                    }
                }
            } else if auth_running {
                div { class: "auth-prompt",
                    div {
                        strong { "Epic authentication" }
                        span { "Starting" }
                    }
                    code { "..." }
                    div { class: "auth-actions",
                        button {
                            class: "secondary-button",
                            onclick: move |_| oncancelauth.call(()),
                            "Cancel"
                        }
                    }
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
                                if account.saved_auth {
                                    span { class: "badge", "Saved login" }
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
                                if account.platform == "Epic" {
                                    button {
                                        class: "secondary-button",
                                        disabled: auth_running,
                                        onclick: move |_| onauth.call(account.id),
                                        if account.saved_auth {
                                            "Re-authenticate"
                                        } else {
                                            "Authenticate"
                                        }
                                    }
                                }
                                button {
                                    class: "secondary-button",
                                    disabled: !can_remove || auth_running,
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
pub(crate) fn UploadDestinationsView(
    summary: AppSummary,
    onautoupload: EventHandler<bool>,
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

#[component]
pub(crate) fn Metric(label: String, value: String) -> Element {
    rsx! {
        article { class: "metric",
            small { "{label}" }
            strong { "{value}" }
        }
    }
}

const REPO_URL: &str = "https://github.com/rlrml/rlru";

#[component]
pub(crate) fn AboutView() -> Element {
    let version = crate::version::VERSION;
    let commit = crate::version::GIT_COMMIT;
    let commit_short = crate::version::git_commit_short();
    let target = crate::version::TARGET;
    let has_commit = crate::version::has_git_commit();
    let commit_url = format!("{REPO_URL}/commit/{}", commit.trim_end_matches("-dirty"));

    rsx! {
        div { class: "summary-grid",
            Metric { label: "Version", value: version.to_string() }
            Metric { label: "Commit", value: commit_short.to_string() }
            Metric { label: "Target", value: target.to_string() }
        }
        section { class: "panel",
            div { class: "panel-header",
                h2 { "rlru" }
                span { "Rocket League replay uploader" }
            }
            dl { class: "details",
                dt { "Version" }
                dd { "{version}" }
                dt { "Git commit" }
                dd {
                    if has_commit {
                        a {
                            href: "{commit_url}",
                            target: "_blank",
                            rel: "noopener noreferrer",
                            "{commit}"
                        }
                    } else {
                        "unknown"
                    }
                }
                dt { "Build target" }
                dd { "{target}" }
                dt { "Repository" }
                dd {
                    a {
                        href: "{REPO_URL}",
                        target: "_blank",
                        rel: "noopener noreferrer",
                        "{REPO_URL}"
                    }
                }
            }
        }
    }
}
