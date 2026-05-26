use dioxus::prelude::*;

use crate::model::*;
use crate::ActiveView;

#[component]
pub(crate) fn Sidebar(active: ActiveView, onselect: EventHandler<ActiveView>) -> Element {
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
pub(crate) fn HistoryView(
    history: Option<Result<Vec<HistoryRow>, String>>,
    message: String,
    backfill_running: bool,
    active_upload: Option<ActiveUpload>,
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
                                                match history_upload_control(
                                                    &destination,
                                                    &row.match_id,
                                                    active_upload.as_ref(),
                                                    &failed_uploads,
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
                },
            }
        }
    }
}

#[component]
pub(crate) fn AccountsView(
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
