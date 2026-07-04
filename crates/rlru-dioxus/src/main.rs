// Release builds on Windows are a GUI app; opt out of the console subsystem so
// launching the executable does not pop a terminal window behind it. Debug
// builds keep the console so tracing/eprintln output stays visible.
#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

use dioxus::dioxus_core::Task;
use dioxus::prelude::*;
#[cfg(not(target_arch = "wasm32"))]
use tracing_subscriber::EnvFilter;

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
    init_tracing();
    launch_app();
}

#[cfg(not(target_arch = "wasm32"))]
fn init_tracing() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();
}

#[cfg(target_arch = "wasm32")]
fn init_tracing() {
    // `tracing_subscriber::fmt` timestamps use `SystemTime`, which panics in browser WASM.
}

#[cfg(feature = "desktop")]
fn launch_app() {
    desktop::launch_app();
}

#[cfg(not(feature = "desktop"))]
fn launch_app() {
    dioxus::launch(App);
}

#[derive(Clone, Copy)]
struct UploadQueueSignals {
    active_uploads: Signal<Vec<ActiveUpload>>,
    running: Signal<bool>,
    completed: Signal<usize>,
    total: Signal<usize>,
    sync_run: Signal<SyncRunState>,
    message: Signal<String>,
    failed_uploads: Signal<Vec<ReplayUploadRequest>>,
    history_refresh_tick: Signal<u64>,
}

fn enqueue_uploads(requests: Vec<ReplayUploadRequest>, queue: UploadQueueSignals) {
    let mut active_uploads = queue.active_uploads;
    let mut upload_queue_running = queue.running;
    let mut upload_queue_completed = queue.completed;
    let mut upload_queue_total = queue.total;
    let mut sync_run = queue.sync_run;
    let mut history_message = queue.message;
    let mut active = active_uploads();
    let mut added = 0_usize;

    for request in requests {
        if active
            .iter()
            .any(|upload| is_same_upload_request(&upload.request, &request))
        {
            continue;
        }
        active.push(ActiveUpload::pending(request));
        added += 1;
    }

    if added == 0 {
        history_message.set("No new uploads to queue".to_string());
        return;
    }

    let was_running = upload_queue_running();
    active_uploads.set(active);
    if was_running {
        upload_queue_total.set(upload_queue_total().saturating_add(added));
    } else {
        upload_queue_completed.set(0);
        upload_queue_total.set(added);
        upload_queue_running.set(true);
        sync_run.set(sync_run().started(now_label()));
        spawn_upload_queue(queue);
    }

    history_message.set(format!("Queued {}", upload_count_label(added)));
}

fn spawn_upload_queue(queue: UploadQueueSignals) {
    let mut active_uploads = queue.active_uploads;
    let mut upload_queue_running = queue.running;
    let mut upload_queue_completed = queue.completed;
    let upload_queue_total = queue.total;
    let mut sync_run = queue.sync_run;
    let mut history_message = queue.message;
    let mut failed_uploads = queue.failed_uploads;
    let mut history_refresh_tick = queue.history_refresh_tick;

    spawn(async move {
        let mut aggregate = BackfillSummary::default();

        // Drain the queue in batches: one upload_history_replays call handles
        // every pending request in a single sync pass per target, so a large
        // retry queue costs one PsyNet login per account instead of one per
        // match (rapid repeated logins trip PsyNet's LoginBanned throttling).
        loop {
            let batch = queued_upload_requests(&active_uploads());
            if batch.is_empty() {
                break;
            }
            for request in &batch {
                mark_uploading(&mut active_uploads, request);
            }
            history_message.set(batch_start_message(&batch));

            let completed = run_upload_batch(batch).await;

            let persisted_failures =
                update_failed_uploads(&mut failed_uploads, &completed.requests, &completed.summary);
            persist_failed_uploads(&persisted_failures);
            for request in &completed.requests {
                if upload_should_wait_for_history(request, &completed.summary) {
                    mark_refreshing(&mut active_uploads, request);
                } else {
                    remove_active_upload(&mut active_uploads, request);
                }
            }
            let completed_count = completed.requests.len();
            merge_backfill_summary(&mut aggregate, completed.summary);
            upload_queue_completed.set(upload_queue_completed().saturating_add(completed_count));
            history_refresh_tick.set(history_refresh_tick().wrapping_add(1));
            history_message.set(format!(
                "Completed {} of {} queued uploads",
                upload_queue_completed(),
                upload_queue_total()
            ));
        }

        upload_queue_running.set(false);
        sync_run.set(sync_run().completed(now_label(), aggregate.clone()));
        history_message.set(append_sync_errors(
            format_backfill_message(
                format!(
                    "Upload queue complete: {} uploaded, {} duplicates, {} cached, {} failed",
                    aggregate.uploaded, aggregate.duplicates, aggregate.cached, aggregate.failed
                ),
                &aggregate.failed_uploads,
            ),
            &aggregate.sync_errors,
        ));
    });
}

fn queued_upload_requests(active_uploads: &[ActiveUpload]) -> Vec<ReplayUploadRequest> {
    active_uploads
        .iter()
        .filter(|upload| upload.phase == UploadPhase::Pending)
        .map(|upload| upload.request.clone())
        .collect()
}

fn batch_start_message(batch: &[ReplayUploadRequest]) -> String {
    match batch {
        [request] => format!(
            "Downloading replay, fetching metadata, and uploading {} to {}",
            short_match_id(&request.match_id),
            request.target_name
        ),
        _ => format!(
            "Syncing accounts and uploading {} in one pass",
            upload_count_label(batch.len())
        ),
    }
}

async fn run_upload_batch(requests: Vec<ReplayUploadRequest>) -> CompletedBatch {
    let summary = match upload_history_replays(requests.clone()).await {
        Ok(summary) => summary,
        Err(error) => {
            tracing::warn!(
                request_count = requests.len(),
                %error,
                "upload batch failed before replay uploads completed"
            );
            failed_batch_summary(&requests, error)
        }
    };
    log_upload_failures(&summary);
    CompletedBatch { requests, summary }
}

fn failed_batch_summary(requests: &[ReplayUploadRequest], reason: String) -> BackfillSummary {
    BackfillSummary {
        failed: requests.len(),
        failed_match_ids: requests
            .iter()
            .map(|request| request.match_id.clone())
            .collect(),
        failed_uploads: requests
            .iter()
            .map(|request| ReplayUploadRequest {
                target_name: request.target_name.clone(),
                match_id: request.match_id.clone(),
                reason: Some(reason.clone()),
            })
            .collect(),
        ..BackfillSummary::default()
    }
}

struct CompletedBatch {
    requests: Vec<ReplayUploadRequest>,
    summary: BackfillSummary,
}

fn mark_uploading(active_uploads: &mut Signal<Vec<ActiveUpload>>, request: &ReplayUploadRequest) {
    let updated = active_uploads()
        .into_iter()
        .map(|upload| {
            if is_same_upload_request(&upload.request, request) {
                ActiveUpload::uploading(upload.request)
            } else {
                upload
            }
        })
        .collect();
    active_uploads.set(updated);
}

fn mark_refreshing(active_uploads: &mut Signal<Vec<ActiveUpload>>, request: &ReplayUploadRequest) {
    let updated = active_uploads()
        .into_iter()
        .map(|upload| {
            if is_same_upload_request(&upload.request, request) {
                ActiveUpload::refreshing(upload.request)
            } else {
                upload
            }
        })
        .collect();
    active_uploads.set(updated);
}

fn remove_active_upload(
    active_uploads: &mut Signal<Vec<ActiveUpload>>,
    request: &ReplayUploadRequest,
) {
    let mut updated = active_uploads();
    updated.retain(|upload| !is_same_upload_request(&upload.request, request));
    active_uploads.set(updated);
}

fn update_failed_uploads(
    failed_uploads: &mut Signal<Vec<ReplayUploadRequest>>,
    requests: &[ReplayUploadRequest],
    run_summary: &BackfillSummary,
) -> Vec<ReplayUploadRequest> {
    let mut failures = failed_uploads();
    failures.retain(|failure| {
        !requests
            .iter()
            .any(|request| is_same_upload_request(failure, request))
    });
    for failure in &run_summary.failed_uploads {
        upsert_failed_upload(&mut failures, failure.clone());
    }
    failed_uploads.set(failures.clone());
    failures
}

fn persist_failed_uploads(failures: &[ReplayUploadRequest]) {
    if let Err(error) = save_persisted_failed_uploads(failures) {
        tracing::warn!(%error, "failed to persist upload failures");
    }
}

fn load_initial_failed_uploads() -> Vec<ReplayUploadRequest> {
    match load_persisted_failed_uploads() {
        Ok(failures) => failures,
        Err(error) => {
            tracing::warn!(%error, "failed to load persisted upload failures");
            Vec::new()
        }
    }
}

fn log_upload_failures(summary: &BackfillSummary) {
    for failure in &summary.failed_uploads {
        let reason = failure.reason.as_deref().unwrap_or("unknown failure");
        tracing::warn!(
            target_name = %failure.target_name,
            match_id = %failure.match_id,
            reason,
            "upload request recorded failed upload"
        );
    }
}

fn upload_should_wait_for_history(
    request: &ReplayUploadRequest,
    run_summary: &BackfillSummary,
) -> bool {
    let completed = run_summary.uploaded + run_summary.duplicates + run_summary.cached > 0;
    let failed = run_summary
        .failed_uploads
        .iter()
        .any(|failure| is_same_upload_request(failure, request));

    completed && !failed
}

fn upload_count_label(count: usize) -> String {
    if count == 1 {
        "1 upload".to_string()
    } else {
        format!("{count} uploads")
    }
}

#[component]
fn App() -> Element {
    let mut summary = use_signal(load_summary);
    let mut active_view = use_signal(|| ActiveView::Overview);
    let mut mobile_nav_open = use_signal(|| false);
    let mut history_requested = use_signal(|| false);
    let mut history_refresh_tick = use_signal(|| 0_u64);
    let mut last_history_rows = use_signal(|| None::<Vec<HistoryRow>>);
    let mut history_error = use_signal(|| None::<String>);
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
    let mut active_uploads = use_signal(Vec::<ActiveUpload>::new);
    let upload_queue_running = use_signal(|| false);
    let upload_queue_completed = use_signal(|| 0_usize);
    let upload_queue_total = use_signal(|| 0_usize);
    let mut sync_run = use_signal(SyncRunState::default);
    let mut failed_uploads = use_signal(load_initial_failed_uploads);
    let account_auth_prompt = use_signal(|| None::<AccountAuthPrompt>);
    let account_auth_running = use_signal(|| false);
    let account_auth_task = use_signal(|| None::<Task>);
    let account_auth_attempt = use_signal(|| 0_u64);
    let upload_queue = UploadQueueSignals {
        active_uploads,
        running: upload_queue_running,
        completed: upload_queue_completed,
        total: upload_queue_total,
        sync_run,
        message: history_message,
        failed_uploads,
        history_refresh_tick,
    };
    let active = active_view();
    let mobile_nav_is_open = mobile_nav_open();
    let current_summary = summary();
    let message = action_message();
    let history_has_been_requested = history_requested();
    let latest_history = history.cloned();
    let cached_history = last_history_rows();
    let current_history = if history_has_been_requested || active == ActiveView::History {
        match latest_history.clone() {
            Some(Ok(rows)) => Some(Ok(rows)),
            Some(Err(error)) => match cached_history.clone() {
                Some(rows) => Some(Ok(rows)),
                None => Some(Err(error)),
            },
            None => cached_history.map(Ok),
        }
    } else {
        Some(Ok(Vec::new()))
    };
    let explicit_history_status = history_message();
    let history_status = if explicit_history_status.is_empty() {
        history_error()
            .map(|error| format!("History refresh failed: {error}"))
            .unwrap_or_default()
    } else {
        explicit_history_status
    };
    let is_backfill_running = backfill_running();
    let current_active_uploads = active_uploads();
    let current_upload_queue_completed = upload_queue_completed();
    let current_upload_queue_total = upload_queue_total();
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

    use_effect(move || match history.cloned() {
        Some(Ok(rows)) => {
            if last_history_rows() != Some(rows.clone()) {
                last_history_rows.set(Some(rows.clone()));
            }
            if history_error().is_some() {
                history_error.set(None);
            }

            let current = active_uploads();
            let reconciled = reconcile_active_uploads_with_history(&current, &rows);
            if reconciled != current {
                active_uploads.set(reconciled);
            }
        }
        Some(Err(error)) if history_error() != Some(error.clone()) => {
            history_error.set(Some(error));
        }
        Some(Err(_)) => {}
        None => {}
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
                            let failures = dedupe_upload_requests(run_summary.failed_uploads.clone());
                            log_upload_failures(&run_summary);
                            failed_uploads.set(failures.clone());
                            persist_failed_uploads(&failures);
                            sync_run.set(sync_run().completed(now_label(), run_summary.clone()));
                            history_message.set(append_sync_errors(
                                format_backfill_message(
                                    format!(
                                        "Sync complete: {} uploaded, {} duplicates, {} cached, {} failed",
                                        run_summary.uploaded,
                                        run_summary.duplicates,
                                        run_summary.cached,
                                        run_summary.failed
                                    ),
                                    &run_summary.failed_uploads,
                                ),
                                &run_summary.sync_errors,
                            ));
                            history_refresh_tick.set(history_refresh_tick().wrapping_add(1));
                        }
                        Err(error) => {
                            tracing::warn!(%error, "sync upload destination run failed");
                            sync_run.set(sync_run().failed(now_label(), error.clone()));
                            history_message.set(error);
                        }
                    }
                    backfill_running.set(false);
                });
            },
            onrefreshhistory: move |_| {
                history_requested.set(true);
                history_error.set(None);
                history_refresh_tick.set(history_refresh_tick().wrapping_add(1));
                history_message.set(String::new());
            },
            onretry: move |request: ReplayUploadRequest| {
                enqueue_uploads(vec![request], upload_queue);
            },
        }
        main {
            class: "shell",
            Sidebar {
                active,
                open: mobile_nav_is_open,
                onselect: move |view| {
                    active_view.set(view);
                    mobile_nav_open.set(false);
                },
                ontoggle: move |_| mobile_nav_open.set(!mobile_nav_open()),
            }
            if mobile_nav_is_open {
                button {
                    class: "nav-backdrop",
                    r#type: "button",
                    aria_label: "Close navigation",
                    onclick: move |_| mobile_nav_open.set(false),
                }
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
                            active_uploads: current_active_uploads,
                            queue_completed: current_upload_queue_completed,
                            queue_total: current_upload_queue_total,
                            failed_uploads: current_failed_uploads,
                            onrefresh: move |_| {
                                history_requested.set(true);
                                history_error.set(None);
                                history_refresh_tick.set(history_refresh_tick().wrapping_add(1));
                                history_message.set(String::new());
                            },
                            onbackfill: move |requests: Vec<ReplayUploadRequest>| {
                                enqueue_uploads(requests, upload_queue);
                            },
                            onupload: move |request: ReplayUploadRequest| {
                                enqueue_uploads(vec![request], upload_queue);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn request(match_id: &str) -> ReplayUploadRequest {
        ReplayUploadRequest {
            target_name: "Rocket Sense".to_string(),
            match_id: match_id.to_string(),
            reason: None,
        }
    }

    #[test]
    fn queued_upload_requests_selects_only_pending_work() {
        let pending = request("pending-match");
        let also_pending = request("second-pending-match");
        let active_uploads = vec![
            ActiveUpload::uploading(request("running-match")),
            ActiveUpload::refreshing(request("refreshing-match")),
            ActiveUpload::pending(pending.clone()),
            ActiveUpload::pending(also_pending.clone()),
        ];

        assert_eq!(
            queued_upload_requests(&active_uploads),
            vec![pending, also_pending]
        );
    }

    #[test]
    fn queued_upload_requests_empty_without_pending_work() {
        let active_uploads = vec![
            ActiveUpload::uploading(request("running-match")),
            ActiveUpload::refreshing(request("refreshing-match")),
        ];

        assert!(queued_upload_requests(&active_uploads).is_empty());
    }

    #[test]
    fn failed_batch_summary_marks_every_request_failed() {
        let requests = vec![request("match-1"), request("match-2")];

        let summary = failed_batch_summary(&requests, "PsyNet down".to_string());

        assert_eq!(summary.failed, 2);
        assert_eq!(summary.uploaded, 0);
        assert_eq!(summary.failed_match_ids, vec!["match-1", "match-2"]);
        assert!(summary
            .failed_uploads
            .iter()
            .all(|failure| failure.reason.as_deref() == Some("PsyNet down")));
    }
}
