use crate::config::{PlayerPlatform, TargetAuth};

use super::{AppSummary, ReplayUploadRequest, SyncRunState};

pub fn short_match_id(match_id: &str) -> &str {
    match_id.get(..8).unwrap_or(match_id)
}

pub fn now_label() -> String {
    chrono::Local::now()
        .format("%Y-%m-%d %H:%M:%S %Z")
        .to_string()
}

pub fn format_backfill_message(message: String, failed_uploads: &[ReplayUploadRequest]) -> String {
    if failed_uploads.is_empty() {
        message
    } else {
        let suffix = if failed_uploads.len() == 1 {
            String::new()
        } else {
            format!("; {} total blocked/failed uploads", failed_uploads.len())
        };
        format!(
            "{message}; first issue: {}{suffix}",
            format_failed_upload(&failed_uploads[0])
        )
    }
}

/// Appends a short account-level sync-error suffix to a run message. These are
/// distinct from per-match upload failures: they explain why a run may have seen
/// fewer (or zero) matches — e.g. an account failed to authenticate or PsyNet
/// was unreachable. Surfacing them keeps a partially-failed run from looking
/// like a clean success.
pub fn append_sync_errors(message: String, sync_errors: &[String]) -> String {
    if sync_errors.is_empty() {
        return message;
    }
    let detail = if sync_errors.len() == 1 {
        format!("account issue: {}", sync_errors[0])
    } else {
        format!(
            "{} account issues; first: {}",
            sync_errors.len(),
            sync_errors[0]
        )
    };
    format!("{message}; {detail}")
}

pub fn dedupe_upload_requests(requests: Vec<ReplayUploadRequest>) -> Vec<ReplayUploadRequest> {
    let mut deduped = Vec::new();
    for request in requests {
        upsert_failed_upload(&mut deduped, request);
    }
    deduped
}

pub fn upsert_failed_upload(
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

pub fn is_same_upload_request(left: &ReplayUploadRequest, right: &ReplayUploadRequest) -> bool {
    is_same_upload(left, &right.target_name, &right.match_id)
}

pub fn is_same_upload(request: &ReplayUploadRequest, target_name: &str, match_id: &str) -> bool {
    request.target_name == target_name && request.match_id == match_id
}

pub fn failed_upload<'a>(
    failed_uploads: &'a [ReplayUploadRequest],
    target_name: &str,
    match_id: &str,
) -> Option<&'a ReplayUploadRequest> {
    failed_uploads
        .iter()
        .find(|failure| is_same_upload(failure, target_name, match_id))
}

pub fn upload_failure_reason(
    failed_uploads: &[ReplayUploadRequest],
    target_name: &str,
    match_id: &str,
) -> String {
    failed_upload(failed_uploads, target_name, match_id)
        .and_then(|failure| failure.reason.clone())
        .unwrap_or_default()
}

pub fn format_failed_upload(failure: &ReplayUploadRequest) -> String {
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

pub fn format_failed_upload_retry_label(failure: &ReplayUploadRequest) -> String {
    format!("Retry {}", format_failed_upload(failure))
}

pub fn auto_upload_label(summary: &AppSummary) -> &'static str {
    if summary.auto_upload {
        "enabled"
    } else {
        "disabled"
    }
}

pub fn tray_sync_label(sync_run: &SyncRunState) -> String {
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

pub fn tray_tooltip(summary: &AppSummary, sync_run: &SyncRunState, failed_count: usize) -> String {
    format!(
        "rlru\n{}\nAuto upload: {}, {}\nFailed uploads: {}",
        tray_sync_label(sync_run),
        auto_upload_label(summary),
        summary.interval,
        failed_count
    )
}

pub(super) fn platform_label(platform: &PlayerPlatform) -> &'static str {
    match platform {
        PlayerPlatform::Epic => "Epic",
        PlayerPlatform::Steam => "Steam",
        PlayerPlatform::PlayStation => "PlayStation",
        PlayerPlatform::Xbox => "Xbox",
        PlayerPlatform::Nintendo => "Nintendo",
    }
}

pub(super) fn auth_label(auth: &TargetAuth) -> String {
    match auth {
        TargetAuth::None => "No auth".to_string(),
        TargetAuth::AuthorizationHeader { .. } => "Authorization header".to_string(),
        TargetAuth::Bearer { .. } => "Bearer token".to_string(),
        TargetAuth::BearerEnv { variable } => {
            if std::env::var_os(variable).is_some() {
                format!("Bearer env token ({variable})")
            } else {
                format!("Bearer env token missing ({variable})")
            }
        }
        TargetAuth::BearerCommand { command } => command
            .first()
            .map(|program| format!("Bearer command token ({program})"))
            .unwrap_or_else(|| "Bearer command token missing command".to_string()),
    }
}

pub(super) fn format_record_start_timestamp(timestamp: i64) -> String {
    use chrono::TimeZone;

    chrono::Local
        .timestamp_opt(timestamp, 0)
        .single()
        .map(|datetime| datetime.format("%Y-%m-%d %H:%M:%S %Z").to_string())
        .unwrap_or_else(|| timestamp.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_sync_errors_no_op_when_empty() {
        assert_eq!(append_sync_errors("done".to_string(), &[]), "done");
    }

    #[test]
    fn append_sync_errors_names_single_issue() {
        let message = append_sync_errors(
            "Sync complete: 0 uploaded".to_string(),
            &["failed to sync account bob: PsyNet timed out".to_string()],
        );
        assert_eq!(
            message,
            "Sync complete: 0 uploaded; account issue: failed to sync account bob: PsyNet timed out"
        );
    }

    #[test]
    fn append_sync_errors_counts_multiple_issues() {
        let message = append_sync_errors(
            "Sync complete".to_string(),
            &["first problem".to_string(), "second problem".to_string()],
        );
        assert_eq!(
            message,
            "Sync complete; 2 account issues; first: first problem"
        );
    }
}
