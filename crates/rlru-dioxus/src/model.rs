#[cfg(not(target_arch = "wasm32"))]
pub(crate) use rlru::app::{
    add_account, backfill_upload_destinations, begin_account_auth, dedupe_upload_requests,
    failed_upload, finish_account_auth, format_backfill_message, is_same_upload,
    is_same_upload_request, load_history, load_persisted_failed_uploads, load_summary, now_label,
    remove_account, save_auto_upload, save_overview_config, save_persisted_failed_uploads,
    short_match_id, upload_failure_reason, upload_history_replay, upsert_failed_upload,
    AccountAuthPrompt, AccountFormData, AppSummary, BackfillSummary, HistoryRow,
    HistoryUploadDestination, OverviewConfigFormData, ReplayUploadRequest, SyncRunState,
    MAX_CONCURRENT_UPLOADS,
};
#[cfg(all(
    not(target_arch = "wasm32"),
    feature = "desktop",
    not(any(target_os = "ios", target_os = "android"))
))]
pub(crate) use rlru::app::{
    auto_upload_label, format_failed_upload_retry_label, tray_sync_label, tray_tooltip,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum UploadPhase {
    Pending,
    Uploading,
    Refreshing,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ActiveUpload {
    pub(crate) request: ReplayUploadRequest,
    pub(crate) phase: UploadPhase,
}

impl ActiveUpload {
    pub(crate) fn pending(request: ReplayUploadRequest) -> Self {
        Self {
            request,
            phase: UploadPhase::Pending,
        }
    }

    pub(crate) fn uploading(request: ReplayUploadRequest) -> Self {
        Self {
            request,
            phase: UploadPhase::Uploading,
        }
    }

    pub(crate) fn refreshing(request: ReplayUploadRequest) -> Self {
        Self {
            request,
            phase: UploadPhase::Refreshing,
        }
    }

    fn is_for(&self, target_name: &str, match_id: &str) -> bool {
        is_same_upload(&self.request, target_name, match_id)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum HistoryUploadControl {
    OpenLink {
        location: String,
    },
    Label {
        class_name: &'static str,
        label: String,
    },
    Button {
        class_name: &'static str,
        label: &'static str,
        title: String,
        disabled: bool,
        request: ReplayUploadRequest,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HistoryUploadActivity {
    pub(crate) class_name: &'static str,
    pub(crate) headline: String,
    pub(crate) detail: String,
    pub(crate) metrics: Vec<HistoryUploadMetric>,
    pub(crate) progress: Option<HistoryUploadProgress>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HistoryUploadMetric {
    pub(crate) label: &'static str,
    pub(crate) value: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HistoryUploadProgress {
    pub(crate) completed: usize,
    pub(crate) total: usize,
    pub(crate) percent: usize,
}

pub(crate) fn history_upload_control(
    destination: &HistoryUploadDestination,
    match_id: &str,
    active_uploads: &[ActiveUpload],
    failed_uploads: &[ReplayUploadRequest],
    batch_running: bool,
) -> HistoryUploadControl {
    if destination.uploaded {
        if let Some(location) = destination.location.clone() {
            return HistoryUploadControl::OpenLink { location };
        }

        if !destination.upload_enabled {
            return HistoryUploadControl::Label {
                class_name: "state-pill uploaded",
                label: destination.state.clone(),
            };
        }
    } else if !destination.upload_enabled {
        return HistoryUploadControl::Label {
            class_name: "state-pill missing",
            label: destination.state.clone(),
        };
    }

    let active_state = active_uploads
        .iter()
        .find(|upload| upload.is_for(&destination.target_name, match_id))
        .map(|upload| &upload.phase);
    let has_failed = failed_upload(failed_uploads, &destination.target_name, match_id).is_some();
    let label = upload_button_label(destination.uploaded, active_state, has_failed);
    let class_name = upload_button_class(active_state);
    let title = upload_button_title(
        active_state,
        failed_uploads,
        &destination.target_name,
        match_id,
        batch_running,
    );

    HistoryUploadControl::Button {
        class_name,
        label,
        title,
        disabled: active_state.is_some() || batch_running,
        request: ReplayUploadRequest {
            target_name: destination.target_name.clone(),
            match_id: match_id.to_string(),
            reason: None,
        },
    }
}

pub(crate) fn history_upload_activity(
    rows: &[HistoryRow],
    active_uploads: &[ActiveUpload],
    failed_uploads: &[ReplayUploadRequest],
    batch_running: bool,
    queue_completed: usize,
    queue_total: usize,
) -> HistoryUploadActivity {
    let counts = HistoryUploadCounts::from_rows(rows, failed_uploads);
    let queued_count = active_uploads
        .iter()
        .filter(|upload| upload.phase == UploadPhase::Pending)
        .count();
    let refreshing_count = active_uploads
        .iter()
        .filter(|upload| upload.phase == UploadPhase::Refreshing)
        .count();
    let running_upload = active_uploads
        .iter()
        .find(|upload| upload.phase == UploadPhase::Uploading);
    let active_count = active_uploads.len();
    let progress = upload_progress(active_count, queue_completed, queue_total);

    let pending_work = counts.pending_work();
    let (class_name, headline, detail) = if batch_running {
        (
            "upload-activity upload-activity-running",
            if pending_work == 0 {
                "Batch upload running".to_string()
            } else {
                format!(
                    "Batch upload running for {}",
                    pluralize(pending_work, "visible upload")
                )
            },
            "Manual uploads are paused until this run finishes.".to_string(),
        )
    } else if let Some(upload) = running_upload {
        (
            "upload-activity upload-activity-running",
            queue_headline(queue_completed, queue_total, active_count),
            format!(
                "Downloading replay, fetching metadata, and uploading {} to {}",
                short_match_id(&upload.request.match_id),
                upload.request.target_name
            ),
        )
    } else if queued_count > 0 {
        (
            "upload-activity upload-activity-running",
            queue_headline(queue_completed, queue_total, active_count),
            format!(
                "{} queued and waiting to start.",
                pluralize(queued_count, "upload")
            ),
        )
    } else if refreshing_count > 0 {
        (
            "upload-activity upload-activity-running",
            queue_headline(queue_completed, queue_total, active_count),
            if refreshing_count == 1 {
                "Refreshing upload status and link.".to_string()
            } else {
                format!(
                    "Refreshing status and links for {}.",
                    pluralize(refreshing_count, "upload")
                )
            },
        )
    } else if counts.visible_failures > 0 {
        (
            "upload-activity upload-activity-needs-attention",
            format!(
                "{} need attention",
                pluralize(counts.visible_failures, "visible upload")
            ),
            ready_upload_detail(pending_work),
        )
    } else if pending_work > 0 {
        (
            "upload-activity",
            format!("{} ready", pluralize(pending_work, "visible upload")),
            "Use Backfill Destinations to queue them, or upload individual rows below.".to_string(),
        )
    } else {
        (
            "upload-activity upload-activity-idle",
            "No visible uploads pending".to_string(),
            "Visible uploaded replays already have cached links.".to_string(),
        )
    };

    HistoryUploadActivity {
        class_name,
        headline,
        detail,
        metrics: vec![
            HistoryUploadMetric {
                label: "Ready",
                value: counts.upload_candidates.to_string(),
            },
            HistoryUploadMetric {
                label: "Need links",
                value: counts.link_candidates.to_string(),
            },
            HistoryUploadMetric {
                label: "Open links",
                value: counts.open_links.to_string(),
            },
            HistoryUploadMetric {
                label: "Queued",
                value: active_count.to_string(),
            },
            HistoryUploadMetric {
                label: "Failed",
                value: counts.visible_failures.to_string(),
            },
        ],
        progress,
    }
}

pub(crate) fn history_upload_requests(
    rows: &[HistoryRow],
    active_uploads: &[ActiveUpload],
) -> Vec<ReplayUploadRequest> {
    let mut requests = Vec::new();
    for row in rows {
        for destination in &row.upload_destinations {
            if !destination.upload_enabled || destination.location.is_some() {
                continue;
            }

            let request = ReplayUploadRequest {
                target_name: destination.target_name.clone(),
                match_id: row.match_id.clone(),
                reason: None,
            };
            if active_uploads
                .iter()
                .any(|upload| is_same_upload_request(&upload.request, &request))
                || requests
                    .iter()
                    .any(|queued| is_same_upload_request(queued, &request))
            {
                continue;
            }
            requests.push(request);
        }
    }
    requests
}

pub(crate) fn reconcile_active_uploads_with_history(
    active_uploads: &[ActiveUpload],
    rows: &[HistoryRow],
) -> Vec<ActiveUpload> {
    active_uploads
        .iter()
        .filter(|upload| {
            upload.phase != UploadPhase::Refreshing
                || !history_contains_uploaded_destination(rows, &upload.request)
        })
        .cloned()
        .collect()
}

pub(crate) fn merge_backfill_summary(summary: &mut BackfillSummary, next: BackfillSummary) {
    summary.uploaded += next.uploaded;
    summary.duplicates += next.duplicates;
    summary.cached += next.cached;
    summary.failed += next.failed;
    summary.failed_match_ids.extend(next.failed_match_ids);
    for failed_upload in next.failed_uploads {
        upsert_failed_upload(&mut summary.failed_uploads, failed_upload);
    }
}

pub(crate) fn failed_upload_summary(
    request: &ReplayUploadRequest,
    reason: String,
) -> BackfillSummary {
    BackfillSummary {
        uploaded: 0,
        duplicates: 0,
        cached: 0,
        failed: 1,
        failed_match_ids: vec![request.match_id.clone()],
        failed_uploads: vec![ReplayUploadRequest {
            target_name: request.target_name.clone(),
            match_id: request.match_id.clone(),
            reason: Some(reason),
        }],
    }
}

fn upload_button_label(
    uploaded_without_location: bool,
    active_state: Option<&UploadPhase>,
    has_failed: bool,
) -> &'static str {
    match (uploaded_without_location, active_state, has_failed) {
        (true, Some(UploadPhase::Pending), _) => "Queued link",
        (true, Some(UploadPhase::Uploading), _) => "Getting link",
        (true, Some(UploadPhase::Refreshing), _) => "Getting link",
        (true, None, true) => "Retry link",
        (true, None, false) => "Get link",
        (false, Some(UploadPhase::Pending), _) => "Queued upload",
        (false, Some(UploadPhase::Uploading), _) => "Uploading",
        (false, Some(UploadPhase::Refreshing), _) => "Getting link",
        (false, None, true) => "Retry upload",
        (false, None, false) => "Upload",
    }
}

fn upload_button_class(active_state: Option<&UploadPhase>) -> &'static str {
    match active_state {
        Some(UploadPhase::Pending) => "compact-button upload-pending",
        Some(UploadPhase::Uploading | UploadPhase::Refreshing) => "compact-button upload-running",
        None => "compact-button",
    }
}

fn upload_button_title(
    active_state: Option<&UploadPhase>,
    failed_uploads: &[ReplayUploadRequest],
    target_name: &str,
    match_id: &str,
    batch_running: bool,
) -> String {
    match active_state {
        Some(UploadPhase::Pending) => "Upload request is queued".to_string(),
        Some(UploadPhase::Uploading) => "Upload is in progress".to_string(),
        Some(UploadPhase::Refreshing) => "Refreshing upload status and link".to_string(),
        None if batch_running => "Batch upload is already running".to_string(),
        None => upload_failure_reason(failed_uploads, target_name, match_id),
    }
}

fn history_contains_uploaded_destination(
    rows: &[HistoryRow],
    request: &ReplayUploadRequest,
) -> bool {
    rows.iter()
        .find(|row| row.match_id == request.match_id)
        .and_then(|row| {
            row.upload_destinations
                .iter()
                .find(|destination| destination.target_name == request.target_name)
        })
        .is_some_and(|destination| destination.uploaded)
}

fn upload_progress(
    active_count: usize,
    queue_completed: usize,
    queue_total: usize,
) -> Option<HistoryUploadProgress> {
    if active_count == 0 || queue_total == 0 {
        return None;
    }

    Some(HistoryUploadProgress {
        completed: queue_completed.min(queue_total),
        total: queue_total,
        percent: queue_completed.saturating_mul(100) / queue_total,
    })
}

fn queue_headline(queue_completed: usize, queue_total: usize, active_count: usize) -> String {
    if queue_total > 1 {
        format!(
            "{} of {} uploads complete",
            queue_completed.min(queue_total),
            queue_total
        )
    } else {
        format!("{} active", pluralize(active_count, "upload"))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct HistoryUploadCounts {
    upload_candidates: usize,
    link_candidates: usize,
    open_links: usize,
    visible_failures: usize,
}

impl HistoryUploadCounts {
    fn from_rows(rows: &[HistoryRow], failed_uploads: &[ReplayUploadRequest]) -> Self {
        let mut counts = Self::default();
        for row in rows {
            for destination in &row.upload_destinations {
                if destination.upload_enabled && !destination.uploaded {
                    counts.upload_candidates += 1;
                } else if destination.upload_enabled
                    && destination.uploaded
                    && destination.location.is_none()
                {
                    counts.link_candidates += 1;
                } else if destination.location.is_some() {
                    counts.open_links += 1;
                }

                if failed_upload(failed_uploads, &destination.target_name, &row.match_id).is_some()
                {
                    counts.visible_failures += 1;
                }
            }
        }
        counts
    }

    fn pending_work(self) -> usize {
        self.upload_candidates + self.link_candidates
    }
}

fn ready_upload_detail(pending_work: usize) -> String {
    if pending_work == 0 {
        "Resolve the failed upload before the visible history is clear.".to_string()
    } else {
        format!(
            "{} still ready after failures are resolved.",
            pluralize(pending_work, "visible upload")
        )
    }
}

fn pluralize(count: usize, noun: &str) -> String {
    if count == 1 {
        format!("1 {noun}")
    } else {
        format!("{count} {noun}s")
    }
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AppSummary {
    pub(crate) config_path: String,
    pub(crate) accounts: Vec<AccountSummary>,
    pub(crate) upload_destinations: Vec<UploadDestinationSummary>,
    pub(crate) auto_upload: bool,
    pub(crate) upload_on_launch: bool,
    pub(crate) no_upload_while_connected: bool,
    pub(crate) selected_account: Option<String>,
    pub(crate) selected_upload_destination: Option<String>,
    pub(crate) auto_upload_interval_minutes: u64,
    pub(crate) auto_upload_jitter_minutes: u64,
    pub(crate) interval: String,
    pub(crate) jitter: String,
    pub(crate) status: String,
}

#[cfg(target_arch = "wasm32")]
impl AppSummary {
    pub(crate) fn account_count(&self) -> usize {
        self.accounts.len()
    }

    pub(crate) fn upload_destination_count(&self) -> usize {
        self.upload_destinations.len()
    }
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AccountSummary {
    pub(crate) id: u32,
    pub(crate) name: String,
    pub(crate) platform: String,
    pub(crate) sync_enabled: bool,
    pub(crate) selected: bool,
    pub(crate) saved_auth: bool,
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AccountFormData {
    pub(crate) name: String,
    pub(crate) platform: String,
    pub(crate) sync_enabled: bool,
    pub(crate) authenticate: bool,
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AccountAuthPrompt {
    pub(crate) account_id: u32,
    pub(crate) account_name: String,
    pub(crate) login_url: String,
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct OverviewConfigFormData {
    pub(crate) auto_upload_interval_minutes: String,
    pub(crate) auto_upload_jitter_minutes: String,
    pub(crate) upload_on_launch: bool,
    pub(crate) no_upload_while_connected: bool,
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct UploadDestinationSummary {
    pub(crate) name: String,
    pub(crate) url: String,
    pub(crate) upload_enabled: bool,
    pub(crate) automatic: bool,
    pub(crate) auth: String,
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HistoryRow {
    pub(crate) account: String,
    pub(crate) match_id: String,
    pub(crate) timestamp: String,
    pub(crate) map_name: String,
    pub(crate) playlist: String,
    pub(crate) score: String,
    pub(crate) upload_destinations: Vec<HistoryUploadDestination>,
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HistoryUploadDestination {
    pub(crate) target_name: String,
    pub(crate) state: String,
    pub(crate) uploaded: bool,
    pub(crate) upload_enabled: bool,
    pub(crate) location: Option<String>,
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ReplayUploadRequest {
    pub(crate) target_name: String,
    pub(crate) match_id: String,
    pub(crate) reason: Option<String>,
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SyncRunState {
    pub(crate) running: bool,
    pub(crate) last_started_at: Option<String>,
    pub(crate) last_completed_at: Option<String>,
    pub(crate) last_summary: Option<BackfillSummary>,
    pub(crate) last_error: Option<String>,
}

#[cfg(target_arch = "wasm32")]
impl SyncRunState {
    pub(crate) fn started(&self, started_at: String) -> Self {
        Self {
            running: true,
            last_started_at: Some(started_at),
            last_completed_at: self.last_completed_at.clone(),
            last_summary: self.last_summary.clone(),
            last_error: None,
        }
    }

    pub(crate) fn completed(&self, completed_at: String, summary: BackfillSummary) -> Self {
        Self {
            running: false,
            last_started_at: self.last_started_at.clone(),
            last_completed_at: Some(completed_at),
            last_summary: Some(summary),
            last_error: None,
        }
    }

    pub(crate) fn failed(&self, completed_at: String, error: String) -> Self {
        Self {
            running: false,
            last_started_at: self.last_started_at.clone(),
            last_completed_at: Some(completed_at),
            last_summary: self.last_summary.clone(),
            last_error: Some(error),
        }
    }
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct BackfillSummary {
    pub(crate) uploaded: usize,
    pub(crate) duplicates: usize,
    pub(crate) cached: usize,
    pub(crate) failed: usize,
    pub(crate) failed_match_ids: Vec<String>,
    pub(crate) failed_uploads: Vec<ReplayUploadRequest>,
}

#[cfg(target_arch = "wasm32")]
pub(crate) const MAX_CONCURRENT_UPLOADS: usize = 3;

#[cfg(target_arch = "wasm32")]
pub(crate) fn now_label() -> String {
    "now".to_string()
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn short_match_id(match_id: &str) -> &str {
    match_id.get(..8).unwrap_or(match_id)
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn format_backfill_message(
    message: String,
    failed_uploads: &[ReplayUploadRequest],
) -> String {
    if failed_uploads.is_empty() {
        message
    } else {
        format!(
            "{message}; first issue: {}",
            format_failed_upload(&failed_uploads[0])
        )
    }
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn dedupe_upload_requests(
    requests: Vec<ReplayUploadRequest>,
) -> Vec<ReplayUploadRequest> {
    let mut deduped = Vec::new();
    for request in requests {
        upsert_failed_upload(&mut deduped, request);
    }
    deduped
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn load_persisted_failed_uploads() -> Result<Vec<ReplayUploadRequest>, String> {
    Ok(Vec::new())
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn save_persisted_failed_uploads(
    _failed_uploads: &[ReplayUploadRequest],
) -> Result<(), String> {
    Ok(())
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn upsert_failed_upload(
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

#[cfg(target_arch = "wasm32")]
pub(crate) fn is_same_upload_request(
    left: &ReplayUploadRequest,
    right: &ReplayUploadRequest,
) -> bool {
    is_same_upload(left, &right.target_name, &right.match_id)
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn is_same_upload(
    request: &ReplayUploadRequest,
    target_name: &str,
    match_id: &str,
) -> bool {
    request.target_name == target_name && request.match_id == match_id
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn failed_upload<'a>(
    failed_uploads: &'a [ReplayUploadRequest],
    target_name: &str,
    match_id: &str,
) -> Option<&'a ReplayUploadRequest> {
    failed_uploads
        .iter()
        .find(|failure| is_same_upload(failure, target_name, match_id))
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn upload_failure_reason(
    failed_uploads: &[ReplayUploadRequest],
    target_name: &str,
    match_id: &str,
) -> String {
    failed_upload(failed_uploads, target_name, match_id)
        .and_then(|failure| failure.reason.clone())
        .unwrap_or_default()
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn format_failed_upload(failure: &ReplayUploadRequest) -> String {
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

#[cfg(target_arch = "wasm32")]
pub(crate) async fn load_history() -> Result<Vec<HistoryRow>, String> {
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
pub(crate) async fn backfill_upload_destinations() -> Result<BackfillSummary, String> {
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
pub(crate) async fn upload_history_replay(
    _request: ReplayUploadRequest,
) -> Result<BackfillSummary, String> {
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
pub(crate) async fn upload_history_replays(
    requests: Vec<ReplayUploadRequest>,
) -> Result<BackfillSummary, String> {
    let uploaded = requests.len();
    Ok(BackfillSummary {
        uploaded,
        duplicates: 0,
        cached: 0,
        failed: 0,
        failed_match_ids: Vec::new(),
        failed_uploads: Vec::new(),
    })
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn load_summary() -> AppSummary {
    AppSummary {
        config_path: "Browser preview uses default local config shape".to_string(),
        accounts: vec![AccountSummary {
            id: 1,
            name: "colonelpanic8".to_string(),
            platform: "Epic".to_string(),
            sync_enabled: true,
            selected: true,
            saved_auth: true,
        }],
        upload_destinations: vec![UploadDestinationSummary {
            name: "Rocket Sense".to_string(),
            url: "https://rocket-sense.duckdns.org/api/v1".to_string(),
            upload_enabled: true,
            automatic: true,
            auth: "Bearer env token".to_string(),
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
pub(crate) fn add_account(input: AccountFormData) -> Result<AppSummary, String> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err("Account name is required".to_string());
    }

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
        platform: platform_preview_label(&input.platform).to_string(),
        sync_enabled: input.sync_enabled,
        selected: false,
        saved_auth: false,
    });
    Ok(summary)
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn begin_account_auth(account_id: u32) -> Result<AccountAuthPrompt, String> {
    Ok(AccountAuthPrompt {
        account_id,
        account_name: "Preview".to_string(),
        login_url: "https://www.epicgames.com/id/login".to_string(),
    })
}

#[cfg(target_arch = "wasm32")]
pub(crate) async fn finish_account_auth(
    prompt: AccountAuthPrompt,
    _code: String,
) -> Result<String, String> {
    Ok(format!("Authenticated {}", prompt.account_name))
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn remove_account(account_id: u32) -> Result<AppSummary, String> {
    let mut summary = load_summary();
    summary.accounts.retain(|account| account.id != account_id);
    Ok(summary)
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn platform_preview_label(value: &str) -> &'static str {
    match value {
        "steam" => "Steam",
        "play_station" => "PlayStation",
        "xbox" => "Xbox",
        "nintendo" => "Nintendo",
        _ => "Epic",
    }
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn save_auto_upload(enabled: bool) -> Result<AppSummary, String> {
    let mut summary = load_summary();
    summary.auto_upload = enabled;
    Ok(summary)
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn save_overview_config(input: OverviewConfigFormData) -> Result<AppSummary, String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    const MATCH_ID: &str = "4E8409F8A8F4431DBF2412B30F2461B5";
    const TARGET_NAME: &str = "Rocket Sense";

    fn destination(uploaded: bool) -> HistoryUploadDestination {
        HistoryUploadDestination {
            target_name: TARGET_NAME.to_string(),
            state: if uploaded {
                "Uploaded".to_string()
            } else {
                "Not uploaded".to_string()
            },
            uploaded,
            upload_enabled: true,
            location: None,
        }
    }

    fn request() -> ReplayUploadRequest {
        ReplayUploadRequest {
            target_name: TARGET_NAME.to_string(),
            match_id: MATCH_ID.to_string(),
            reason: None,
        }
    }

    #[test]
    fn pending_upload_has_distinct_button_state() {
        let active_upload = ActiveUpload::pending(request());

        let control =
            history_upload_control(&destination(false), MATCH_ID, &[active_upload], &[], false);

        assert_eq!(
            control,
            HistoryUploadControl::Button {
                class_name: "compact-button upload-pending",
                label: "Queued upload",
                title: "Upload request is queued".to_string(),
                disabled: true,
                request: request(),
            }
        );
    }

    #[test]
    fn running_upload_has_distinct_button_state() {
        let active_upload = ActiveUpload::uploading(request());

        let control =
            history_upload_control(&destination(false), MATCH_ID, &[active_upload], &[], false);

        assert_eq!(
            control,
            HistoryUploadControl::Button {
                class_name: "compact-button upload-running",
                label: "Uploading",
                title: "Upload is in progress".to_string(),
                disabled: true,
                request: request(),
            }
        );
    }

    #[test]
    fn refreshing_upload_keeps_button_disabled_while_history_is_stale() {
        let active_upload = ActiveUpload::refreshing(request());

        let control =
            history_upload_control(&destination(false), MATCH_ID, &[active_upload], &[], false);

        assert_eq!(
            control,
            HistoryUploadControl::Button {
                class_name: "compact-button upload-running",
                label: "Getting link",
                title: "Refreshing upload status and link".to_string(),
                disabled: true,
                request: request(),
            }
        );
    }

    #[test]
    fn uploaded_without_location_uses_link_specific_labels() {
        let active_upload = ActiveUpload::pending(request());

        let control =
            history_upload_control(&destination(true), MATCH_ID, &[active_upload], &[], false);

        assert_eq!(
            control,
            HistoryUploadControl::Button {
                class_name: "compact-button upload-pending",
                label: "Queued link",
                title: "Upload request is queued".to_string(),
                disabled: true,
                request: request(),
            }
        );
    }

    #[test]
    fn refreshed_history_clears_completed_uploads() {
        let stale_history = vec![HistoryRow {
            account: "Primary".to_string(),
            match_id: MATCH_ID.to_string(),
            timestamp: "now".to_string(),
            map_name: "DFH Stadium".to_string(),
            playlist: "13".to_string(),
            score: "3-2".to_string(),
            upload_destinations: vec![destination(false)],
        }];
        let refreshed_history = vec![HistoryRow {
            upload_destinations: vec![destination(true)],
            ..stale_history[0].clone()
        }];
        let active = vec![
            ActiveUpload::pending(ReplayUploadRequest {
                target_name: "Ballchasing".to_string(),
                match_id: MATCH_ID.to_string(),
                reason: None,
            }),
            ActiveUpload::refreshing(request()),
        ];

        assert_eq!(
            reconcile_active_uploads_with_history(&active, &stale_history),
            active
        );
        assert_eq!(
            reconcile_active_uploads_with_history(&active, &refreshed_history),
            vec![active[0].clone()]
        );
    }

    #[test]
    fn batch_upload_disables_individual_upload_buttons() {
        let control = history_upload_control(&destination(false), MATCH_ID, &[], &[], true);

        assert_eq!(
            control,
            HistoryUploadControl::Button {
                class_name: "compact-button",
                label: "Upload",
                title: "Batch upload is already running".to_string(),
                disabled: true,
                request: request(),
            }
        );
    }

    #[test]
    fn activity_summary_counts_visible_upload_work() {
        let rows = vec![
            HistoryRow {
                account: "Primary".to_string(),
                match_id: MATCH_ID.to_string(),
                timestamp: "now".to_string(),
                map_name: "DFH Stadium".to_string(),
                playlist: "13".to_string(),
                score: "3-2".to_string(),
                upload_destinations: vec![
                    destination(false),
                    HistoryUploadDestination {
                        target_name: "Ballchasing".to_string(),
                        state: "Uploaded".to_string(),
                        uploaded: true,
                        upload_enabled: true,
                        location: None,
                    },
                ],
            },
            HistoryRow {
                account: "Primary".to_string(),
                match_id: "F90812E5EFDA4CC4AC7903596F02E6AB".to_string(),
                timestamp: "now".to_string(),
                map_name: "Mannfield".to_string(),
                playlist: "13".to_string(),
                score: "1-4".to_string(),
                upload_destinations: vec![HistoryUploadDestination {
                    target_name: TARGET_NAME.to_string(),
                    state: "Uploaded".to_string(),
                    uploaded: true,
                    upload_enabled: true,
                    location: Some("https://example.com/replay".to_string()),
                }],
            },
        ];

        let activity = history_upload_activity(&rows, &[], &[request()], true, 0, 0);

        assert_eq!(
            activity.class_name,
            "upload-activity upload-activity-running"
        );
        assert_eq!(
            activity.headline,
            "Batch upload running for 2 visible uploads"
        );
        assert_eq!(
            activity.metrics,
            vec![
                HistoryUploadMetric {
                    label: "Ready",
                    value: "1".to_string(),
                },
                HistoryUploadMetric {
                    label: "Need links",
                    value: "1".to_string(),
                },
                HistoryUploadMetric {
                    label: "Open links",
                    value: "1".to_string(),
                },
                HistoryUploadMetric {
                    label: "Queued",
                    value: "0".to_string(),
                },
                HistoryUploadMetric {
                    label: "Failed",
                    value: "1".to_string(),
                },
            ]
        );
    }
}
