use dioxus::dioxus_core::Task;
use dioxus::prelude::*;

#[cfg(feature = "desktop")]
mod desktop;
mod model;
mod tray;
mod version;
mod views;

use model::*;
use tray::DesktopTrayBridge;
use views::*;

const APP_CSS: &str = include_str!("../assets/styles.css");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ActiveView {
    Overview,
    History,
    Accounts,
    UploadDestinations,
    About,
}

impl ActiveView {
    const ALL: [Self; 5] = [
        Self::Overview,
        Self::History,
        Self::Accounts,
        Self::UploadDestinations,
        Self::About,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::History => "History",
            Self::Accounts => "Accounts",
            Self::UploadDestinations => "Upload Destinations",
            Self::About => "About",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::Overview => "Local auth, typed config, replay upload destinations",
            Self::History => "Current RL API matches and upload destination state",
            Self::Accounts => "Configured Rocket League account credentials",
            Self::UploadDestinations => "Replay destinations, upload mode, and activity",
            Self::About => "Version, build, and project information",
        }
    }
}

fn main() {
    launch_app();
}

#[cfg(feature = "desktop")]
fn launch_app() {
    desktop::launch_app();
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
    let mut active_upload = use_signal(|| None::<ActiveUpload>);
    let mut sync_run = use_signal(SyncRunState::default);
    let mut failed_uploads = use_signal(Vec::<ReplayUploadRequest>::new);
    let account_auth_prompt = use_signal(|| None::<AccountAuthPrompt>);
    let account_auth_running = use_signal(|| false);
    let account_auth_task = use_signal(|| None::<Task>);
    let account_auth_attempt = use_signal(|| 0_u64);
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
    let current_active_upload = active_upload();
    let current_sync_run = sync_run();
    let current_failed_uploads = failed_uploads();
    let current_account_auth_prompt = account_auth_prompt();
    let current_account_auth_running = account_auth_running();
    let show_config_refresh = active != ActiveView::History && active != ActiveView::About;

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
                sync_run.set(sync_run().started(now_label()));
                history_message.set("Syncing upload destinations from current RL API history".to_string());
                spawn(async move {
                    match backfill_upload_destinations().await {
                        Ok(run_summary) => {
                            failed_uploads.set(dedupe_upload_requests(run_summary.failed_uploads.clone()));
                            sync_run.set(sync_run().completed(now_label(), run_summary.clone()));
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
                            sync_run.set(sync_run().failed(now_label(), error.clone()));
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
                active_upload.set(Some(ActiveUpload::pending(request.clone())));
                sync_run.set(sync_run().started(now_label()));
                history_message.set(format!(
                    "Pending retry for {} to {}",
                    short_match_id(&request.match_id),
                    request.target_name
                ));
                spawn(async move {
                    active_upload.set(Some(ActiveUpload::uploading(request.clone())));
                    history_message.set(format!(
                        "Retrying {} to {}",
                        short_match_id(&request.match_id),
                        request.target_name
                    ));
                    match upload_history_replay(request.clone()).await {
                        Ok(run_summary) => {
                            let mut failures = failed_uploads();
                            failures.retain(|failure| !is_same_upload_request(failure, &request));
                            for failure in &run_summary.failed_uploads {
                                upsert_failed_upload(&mut failures, failure.clone());
                            }
                            failed_uploads.set(failures);
                            sync_run.set(sync_run().completed(now_label(), run_summary.clone()));
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
                            sync_run.set(sync_run().failed(now_label(), error.clone()));
                            history_message.set(error);
                        }
                    }
                    active_upload.set(None);
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
                    if show_config_refresh {
                        button {
                            class: "secondary-button",
                            onclick: move |_| {
                                summary.set(load_summary());
                                action_message.set(String::new());
                            },
                            "Reload Config"
                        }
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
                            active_upload: current_active_upload,
                            failed_uploads: current_failed_uploads,
                            onrefresh: move |_| {
                                history_requested.set(true);
                                history_refresh_tick.set(history_refresh_tick().wrapping_add(1));
                                history_message.set(String::new());
                            },
                            onbackfill: move |_| {
                                backfill_running.set(true);
                                sync_run.set(sync_run().started(now_label()));
                                history_message.set("Backfilling upload destinations from current RL API history".to_string());
                                spawn(async move {
                                    match backfill_upload_destinations().await {
                                        Ok(run_summary) => {
                                            failed_uploads.set(dedupe_upload_requests(run_summary.failed_uploads.clone()));
                                            sync_run.set(sync_run().completed(now_label(), run_summary.clone()));
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
                                            sync_run.set(sync_run().failed(now_label(), error.clone()));
                                            history_message.set(error);
                                        }
                                    }
                                    backfill_running.set(false);
                                });
                            },
                            onupload: move |request: ReplayUploadRequest| {
                                active_upload.set(Some(ActiveUpload::pending(request.clone())));
                                sync_run.set(sync_run().started(now_label()));
                                history_message.set(format!(
                                    "Pending upload for {} to {}",
                                    short_match_id(&request.match_id),
                                    request.target_name
                                ));
                                spawn(async move {
                                    active_upload.set(Some(ActiveUpload::uploading(request.clone())));
                                    history_message.set(format!(
                                        "Uploading {} to {}",
                                        short_match_id(&request.match_id),
                                        request.target_name
                                    ));
                                    match upload_history_replay(request.clone()).await {
                                        Ok(run_summary) => {
                                            let mut failures = failed_uploads();
                                            failures.retain(|failure| !is_same_upload_request(failure, &request));
                                            for failure in &run_summary.failed_uploads {
                                                upsert_failed_upload(&mut failures, failure.clone());
                                            }
                                            failed_uploads.set(failures);
                                            sync_run.set(sync_run().completed(now_label(), run_summary.clone()));
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
                                            sync_run.set(sync_run().failed(now_label(), error.clone()));
                                            history_message.set(error);
                                        }
                                    }
                                    active_upload.set(None);
                                });
                            },
                        }
                    },
                    ActiveView::Accounts => rsx! {
                        AccountsView {
                            summary: current_summary,
                            auth_prompt: current_account_auth_prompt,
                            auth_running: current_account_auth_running,
                            onadd: move |input: AccountFormData| {
                                let authenticate = input.authenticate && input.platform == "epic";
                                let account_name = input.name.trim().to_string();
                                match add_account(input) {
                                    Ok(updated) => {
                                        let account_id = updated
                                            .accounts
                                            .iter()
                                            .find(|account| account.name == account_name)
                                            .map(|account| account.id);
                                        summary.set(updated);
                                        if authenticate {
                                            if let Some(account_id) = account_id {
                                                start_account_auth(
                                                    account_id,
                                                    action_message,
                                                    account_auth_prompt,
                                                    account_auth_running,
                                                    account_auth_task,
                                                    account_auth_attempt,
                                                );
                                            } else {
                                                action_message.set("Account added, but it could not be found for authentication".to_string());
                                            }
                                        } else {
                                            action_message.set("Account added to config".to_string());
                                        }
                                    }
                                    Err(error) => action_message.set(error),
                                }
                            },
                            onauth: move |account_id: u32| {
                                start_account_auth(
                                    account_id,
                                    action_message,
                                    account_auth_prompt,
                                    account_auth_running,
                                    account_auth_task,
                                    account_auth_attempt,
                                );
                            },
                            onregenauth: move |account_id: u32| {
                                start_account_auth(
                                    account_id,
                                    action_message,
                                    account_auth_prompt,
                                    account_auth_running,
                                    account_auth_task,
                                    account_auth_attempt,
                                );
                            },
                            onfinishauth: move |(prompt, code): (AccountAuthPrompt, String)| {
                                finish_account_auth_code(
                                    prompt,
                                    code,
                                    AccountAuthState {
                                        summary,
                                        action_message,
                                        prompt: account_auth_prompt,
                                        running: account_auth_running,
                                        task: account_auth_task,
                                        attempt: account_auth_attempt,
                                    },
                                );
                            },
                            oncancelauth: move |_| {
                                cancel_account_auth(
                                    action_message,
                                    account_auth_prompt,
                                    account_auth_running,
                                    account_auth_task,
                                    account_auth_attempt,
                                );
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
                    ActiveView::About => rsx! {
                        AboutView {}
                    },
                }
            }
        }
    }
}

fn start_account_auth(
    account_id: u32,
    mut action_message: Signal<String>,
    mut account_auth_prompt: Signal<Option<AccountAuthPrompt>>,
    mut account_auth_running: Signal<bool>,
    mut account_auth_task: Signal<Option<Task>>,
    mut account_auth_attempt: Signal<u64>,
) {
    if let Some(task) = account_auth_task.take() {
        task.cancel();
    }

    let attempt = account_auth_attempt().wrapping_add(1);
    account_auth_attempt.set(attempt);
    account_auth_running.set(false);
    account_auth_prompt.set(None);

    match begin_account_auth(account_id) {
        Ok(prompt) => {
            if account_auth_attempt() != attempt {
                return;
            }
            account_auth_prompt.set(Some(prompt.clone()));
            action_message.set(format!(
                "Epic authentication ready for {}",
                prompt.account_name
            ));
        }
        Err(error) => action_message.set(error),
    }
}

#[derive(Clone, Copy)]
struct AccountAuthState {
    summary: Signal<AppSummary>,
    action_message: Signal<String>,
    prompt: Signal<Option<AccountAuthPrompt>>,
    running: Signal<bool>,
    task: Signal<Option<Task>>,
    attempt: Signal<u64>,
}

fn finish_account_auth_code(prompt: AccountAuthPrompt, code: String, mut state: AccountAuthState) {
    if let Some(task) = state.task.take() {
        task.cancel();
    }

    let attempt = (state.attempt)().wrapping_add(1);
    state.attempt.set(attempt);
    state.running.set(true);
    state.action_message.set(format!(
        "Finishing Epic authentication for {}",
        prompt.account_name
    ));

    let task = spawn(async move {
        match finish_account_auth(prompt, code).await {
            Ok(message) => {
                if (state.attempt)() != attempt {
                    return;
                }
                state.prompt.set(None);
                state.summary.set(load_summary());
                state.action_message.set(message);
            }
            Err(error) => {
                if (state.attempt)() != attempt {
                    return;
                }
                state.action_message.set(error);
            }
        }
        if (state.attempt)() == attempt {
            state.running.set(false);
            state.task.set(None);
        }
    });
    state.task.set(Some(task));
}

fn cancel_account_auth(
    mut action_message: Signal<String>,
    mut account_auth_prompt: Signal<Option<AccountAuthPrompt>>,
    mut account_auth_running: Signal<bool>,
    mut account_auth_task: Signal<Option<Task>>,
    mut account_auth_attempt: Signal<u64>,
) {
    account_auth_attempt.set(account_auth_attempt().wrapping_add(1));
    if let Some(task) = account_auth_task.take() {
        task.cancel();
    }
    account_auth_prompt.set(None);
    account_auth_running.set(false);
    action_message.set("Epic authentication canceled".to_string());
}
