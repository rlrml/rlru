use dioxus::prelude::*;

const APP_CSS: &str = include_str!("../assets/styles.css");

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
        document::Style { "{APP_CSS}" }
        main { class: "shell",
            Sidebar {}
            section { class: "workspace",
                header { class: "topbar",
                    div {
                        h1 { "Replay Uploader" }
                        p { "Local auth, typed config, replay upload targets" }
                    }
                    button {
                        class: "primary-button",
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
        aside { class: "sidebar",
            strong { "rlru" }
            nav {
                button { class: "nav-button selected", "Overview" }
                button { class: "nav-button", "Accounts" }
                button { class: "nav-button", "Storage" }
                button { class: "nav-button", "Activity" }
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
        div { class: "summary-grid",
            Metric { label: "Accounts", value: summary.account_count.to_string() }
            Metric { label: "Upload Targets", value: summary.storage_count.to_string() }
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
                dd { "Ready for auth and uploader wiring" }
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
fn Metric(label: String, value: String) -> Element {
    rsx! {
        article { class: "metric",
            small { "{label}" }
            strong { "{value}" }
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
        storage_count: 2,
        auto_upload: true,
        interval: "Every 45 minutes".to_string(),
    }
}
