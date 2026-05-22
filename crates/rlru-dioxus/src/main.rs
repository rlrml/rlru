use dioxus::prelude::*;

const APP_CSS: Asset = asset!("/assets/styles.css");
const SHELL_STYLE: &str =
    "display:grid;grid-template-columns:224px minmax(0,1fr);min-height:100vh;width:100%;\
     background:#f3f6f4;color:#18242b;font-family:Inter,ui-sans-serif,system-ui,-apple-system,\
     BlinkMacSystemFont,'Segoe UI',sans-serif;";
const SIDEBAR_STYLE: &str =
    "background:#fff;border-right:1px solid #d8e0dd;display:flex;flex-direction:column;\
     gap:22px;padding:22px 16px;";
const NAV_STYLE: &str = "display:flex;flex-direction:column;gap:6px;";
const NAV_BUTTON_STYLE: &str =
    "background:transparent;border:0;border-radius:6px;color:#2b3941;cursor:pointer;\
     font:inherit;font-weight:700;min-height:38px;padding:0 10px;text-align:left;";
const NAV_BUTTON_SELECTED_STYLE: &str =
    "background:#e6efec;border:0;border-radius:6px;color:#176070;cursor:pointer;\
     font:inherit;font-weight:700;min-height:38px;padding:0 10px;text-align:left;";
const WORKSPACE_STYLE: &str = "min-width:0;padding:26px;";
const TOPBAR_STYLE: &str =
    "align-items:center;display:flex;gap:16px;justify-content:space-between;margin-bottom:18px;";
const MUTED_STYLE: &str = "color:#60727c;";
const PRIMARY_BUTTON_STYLE: &str =
    "background:#1d7187;border:0;border-radius:6px;color:white;cursor:pointer;font:inherit;\
     font-weight:700;min-height:40px;padding:0 18px;";
const SUMMARY_GRID_STYLE: &str =
    "display:grid;gap:16px;grid-template-columns:repeat(3,minmax(0,1fr));margin-bottom:16px;";
const SURFACE_STYLE: &str = "background:#fff;border:1px solid #d8e0dd;border-radius:8px;";
const METRIC_STYLE: &str =
    "background:#fff;border:1px solid #d8e0dd;border-radius:8px;display:flex;\
     flex-direction:column;gap:8px;min-width:0;padding:16px;";
const PANEL_STYLE: &str =
    "background:#fff;border:1px solid #d8e0dd;border-radius:8px;margin-bottom:16px;padding:18px;";
const PANEL_HEADER_STYLE: &str =
    "align-items:center;display:flex;gap:12px;justify-content:space-between;margin-bottom:14px;";
const DETAILS_STYLE: &str =
    "display:grid;gap:8px 16px;grid-template-columns:max-content minmax(0,1fr);margin:0;";
const ACTIVITY_ROW_STYLE: &str = "align-items:center;display:flex;gap:12px;";
const STATUS_DOT_STYLE: &str =
    "background:#2e9d71;border-radius:50%;flex:0 0 auto;height:10px;width:10px;";

#[derive(Clone, Debug, PartialEq)]
struct AppSummary {
    config_path: String,
    account_count: usize,
    storage_count: usize,
    auto_upload: bool,
    interval: String,
}

fn main() {
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    let mut summary = use_signal(load_summary);

    rsx! {
        document::Title { "rlru" }
        document::Meta {
            name: "viewport",
            content: "width=device-width, initial-scale=1, viewport-fit=cover",
        }
        document::Stylesheet { href: APP_CSS }
        main { class: "shell", style: SHELL_STYLE,
            Sidebar {}
            section { class: "workspace", style: WORKSPACE_STYLE,
                header { class: "topbar", style: TOPBAR_STYLE,
                    div {
                        h1 { style: "font-size:1.55rem;margin:0;", "Replay Uploader" }
                        p { style: "{MUTED_STYLE}margin:0;", "Local auth, typed config, replay upload targets" }
                    }
                    button {
                        class: "primary-button",
                        style: PRIMARY_BUTTON_STYLE,
                        onclick: move |_| summary.set(load_summary()),
                        "Refresh"
                    }
                }
                Dashboard { summary: summary() }
            }
        }
    }
}

#[component]
fn Sidebar() -> Element {
    rsx! {
        aside { class: "sidebar", style: SIDEBAR_STYLE,
            strong { style: "font-size:1.15rem;", "rlru" }
            nav { style: NAV_STYLE,
                button { class: "nav-button selected", style: NAV_BUTTON_SELECTED_STYLE, "Overview" }
                button { class: "nav-button", style: NAV_BUTTON_STYLE, "Accounts" }
                button { class: "nav-button", style: NAV_BUTTON_STYLE, "Storage" }
                button { class: "nav-button", style: NAV_BUTTON_STYLE, "Activity" }
            }
        }
    }
}

#[component]
fn Dashboard(summary: AppSummary) -> Element {
    let auto_upload_value = if summary.auto_upload {
        "Enabled"
    } else {
        "Disabled"
    }
    .to_string();

    rsx! {
        div { class: "summary-grid", style: SUMMARY_GRID_STYLE,
            Metric { label: "Accounts", value: summary.account_count.to_string() }
            Metric { label: "Upload Targets", value: summary.storage_count.to_string() }
            Metric { label: "Auto Upload", value: auto_upload_value }
        }
        section { class: "panel", style: PANEL_STYLE,
            div { class: "panel-header", style: PANEL_HEADER_STYLE,
                h2 { style: "font-size:1.05rem;margin:0;", "Configuration" }
                span { style: MUTED_STYLE, "{summary.interval}" }
            }
            dl { class: "details", style: DETAILS_STYLE,
                dt { style: "{MUTED_STYLE}font-weight:700;", "Path" }
                dd { style: "margin:0;min-width:0;overflow-wrap:anywhere;", "{summary.config_path}" }
                dt { style: "{MUTED_STYLE}font-weight:700;", "State" }
                dd { style: "margin:0;min-width:0;overflow-wrap:anywhere;", "Ready for auth, sync, and uploader runs" }
            }
        }
        section { class: "panel", style: PANEL_STYLE,
            div { class: "panel-header", style: PANEL_HEADER_STYLE,
                h2 { style: "font-size:1.05rem;margin:0;", "Sync Pipeline" }
                span { style: MUTED_STYLE, "PsyNet replay discovery" }
            }
            div { class: "activity-row", style: ACTIVITY_ROW_STYLE,
                div { class: "status-dot", style: STATUS_DOT_STYLE }
                p { style: "margin:0;", "Auth, PsyNet match history, replay download, upload, and cache handling are wired behind the CLI/library APIs." }
            }
        }
    }
}

#[component]
fn Metric(label: String, value: String) -> Element {
    rsx! {
        article { class: "metric", style: "{SURFACE_STYLE}{METRIC_STYLE}",
            small { style: MUTED_STYLE, "{label}" }
            strong { style: "font-size:1.45rem;", "{value}" }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn load_summary() -> AppSummary {
    use rlru::paths::AppPaths;
    use rlru::Config;

    match AppPaths::discover() {
        Ok(paths) => {
            let config_path = paths.config_file();
            let config = Config::load_or_default(&config_path).unwrap_or_default();
            AppSummary {
                config_path: config_path.display().to_string(),
                account_count: config.accounts.len(),
                storage_count: config.storage.len(),
                auto_upload: config.behavior.auto_upload,
                interval: format!(
                    "Every {} minutes",
                    config.behavior.auto_upload_interval.as_secs() / 60
                ),
            }
        }
        Err(error) => AppSummary {
            config_path: error.to_string(),
            account_count: 0,
            storage_count: 0,
            auto_upload: false,
            interval: "Unavailable".to_string(),
        },
    }
}

#[cfg(target_arch = "wasm32")]
fn load_summary() -> AppSummary {
    AppSummary {
        config_path: "Browser preview uses default local config shape".to_string(),
        account_count: 1,
        storage_count: 3,
        auto_upload: true,
        interval: "Every 45 minutes".to_string(),
    }
}
