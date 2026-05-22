use dioxus::prelude::*;

const APP_CSS: &str = include_str!("../assets/styles.css");

#[cfg(feature = "desktop")]
fn desktop_head() -> String {
    format!("<style>{APP_CSS}</style>")
}

#[derive(Clone, Debug, PartialEq)]
struct AppSummary {
    config_path: String,
    account_count: usize,
    storage_count: usize,
    auto_upload: bool,
    interval: String,
}

fn main() {
    launch_app();
}

#[cfg(feature = "desktop")]
fn launch_app() {
    use dioxus::desktop::{Config, WindowBuilder};

    dioxus::LaunchBuilder::desktop()
        .with_cfg(
            Config::new()
                .with_custom_head(desktop_head())
                .with_background_color((243, 246, 244, 255))
                .with_window(WindowBuilder::new().with_title("rlru")),
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

    rsx! {
        document::Title { "rlru" }
        document::Meta {
            name: "viewport",
            content: "width=device-width, initial-scale=1, viewport-fit=cover",
        }
        document::Style { "{APP_CSS}" }
        main {
            class: "shell",
            style: "display:grid;grid-template-columns:224px minmax(0,1fr);min-height:100vh;width:100%;background:#f3f6f4;color:#18242b;font-family:Inter,ui-sans-serif,system-ui,-apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif;font-size:16px;",
            Sidebar {}
            section {
                class: "workspace",
                style: "min-width:0;padding:26px;",
                header {
                    class: "topbar",
                    style: "align-items:center;display:flex;gap:16px;justify-content:space-between;margin-bottom:18px;",
                    div {
                        h1 { style: "font-size:1.55rem;margin:0;", "Replay Uploader" }
                        p { style: "color:#60727c;margin:0;", "Local auth, typed config, replay upload targets" }
                    }
                    button {
                        class: "primary-button",
                        style: "background:#1d7187;border:0;border-radius:6px;color:white;cursor:pointer;font:inherit;font-weight:700;min-height:40px;padding:0 18px;",
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
        aside {
            class: "sidebar",
            style: "background:#fff;border-right:1px solid #d8e0dd;display:flex;flex-direction:column;gap:22px;padding:22px 16px;",
            strong { style: "font-size:1.15rem;", "rlru" }
            nav { style: "display:flex;flex-direction:column;gap:6px;",
                button { class: "nav-button selected", style: "background:#e6efec;border:0;border-radius:6px;color:#176070;cursor:pointer;font:inherit;font-weight:700;min-height:38px;padding:0 10px;text-align:left;", "Overview" }
                button { class: "nav-button", style: "background:transparent;border:0;border-radius:6px;color:#2b3941;cursor:pointer;font:inherit;font-weight:700;min-height:38px;padding:0 10px;text-align:left;", "Accounts" }
                button { class: "nav-button", style: "background:transparent;border:0;border-radius:6px;color:#2b3941;cursor:pointer;font:inherit;font-weight:700;min-height:38px;padding:0 10px;text-align:left;", "Storage" }
                button { class: "nav-button", style: "background:transparent;border:0;border-radius:6px;color:#2b3941;cursor:pointer;font:inherit;font-weight:700;min-height:38px;padding:0 10px;text-align:left;", "Activity" }
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
        div { class: "summary-grid", style: "display:grid;gap:16px;grid-template-columns:repeat(3,minmax(0,1fr));margin-bottom:16px;",
            Metric { label: "Accounts", value: summary.account_count.to_string() }
            Metric { label: "Upload Targets", value: summary.storage_count.to_string() }
            Metric { label: "Auto Upload", value: auto_upload_value }
        }
        section { class: "panel", style: "background:#fff;border:1px solid #d8e0dd;border-radius:8px;margin-bottom:16px;padding:18px;",
            div { class: "panel-header", style: "align-items:center;display:flex;gap:12px;justify-content:space-between;margin-bottom:14px;",
                h2 { style: "font-size:1.05rem;margin:0;", "Configuration" }
                span { style: "color:#60727c;", "{summary.interval}" }
            }
            dl { class: "details", style: "display:grid;gap:8px 16px;grid-template-columns:max-content minmax(0,1fr);margin:0;",
                dt { style: "color:#60727c;font-weight:700;", "Path" }
                dd { style: "margin:0;min-width:0;overflow-wrap:anywhere;", "{summary.config_path}" }
                dt { style: "color:#60727c;font-weight:700;", "State" }
                dd { style: "margin:0;min-width:0;overflow-wrap:anywhere;", "Ready for auth, sync, and uploader runs" }
            }
        }
        section { class: "panel", style: "background:#fff;border:1px solid #d8e0dd;border-radius:8px;margin-bottom:16px;padding:18px;",
            div { class: "panel-header", style: "align-items:center;display:flex;gap:12px;justify-content:space-between;margin-bottom:14px;",
                h2 { style: "font-size:1.05rem;margin:0;", "Sync Pipeline" }
                span { style: "color:#60727c;", "PsyNet replay discovery" }
            }
            div { class: "activity-row", style: "align-items:center;display:flex;gap:12px;",
                div { class: "status-dot", style: "background:#2e9d71;border-radius:50%;flex:0 0 auto;height:10px;width:10px;" }
                p { style: "margin:0;", "Auth, PsyNet match history, replay download, upload, and cache handling are wired behind the CLI/library APIs." }
            }
        }
    }
}

#[component]
fn Metric(label: String, value: String) -> Element {
    rsx! {
        article { class: "metric", style: "background:#fff;border:1px solid #d8e0dd;border-radius:8px;display:flex;flex-direction:column;gap:8px;min-width:0;padding:16px;",
            small { style: "color:#60727c;", "{label}" }
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
