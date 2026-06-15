#[derive(Clone, Debug, PartialEq)]
pub struct AppSummary {
    pub config_path: String,
    pub accounts: Vec<AccountSummary>,
    pub upload_destinations: Vec<UploadDestinationSummary>,
    pub auto_upload: bool,
    pub upload_on_launch: bool,
    pub no_upload_while_connected: bool,
    pub selected_account: Option<String>,
    pub selected_upload_destination: Option<String>,
    pub auto_upload_interval_minutes: u64,
    pub auto_upload_jitter_minutes: u64,
    pub interval: String,
    pub jitter: String,
    pub status: String,
}

impl AppSummary {
    pub fn account_count(&self) -> usize {
        self.accounts.len()
    }

    pub fn upload_destination_count(&self) -> usize {
        self.upload_destinations.len()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct AccountSummary {
    pub id: u32,
    pub name: String,
    pub platform: String,
    pub sync_enabled: bool,
    pub selected: bool,
    pub saved_auth: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountFormData {
    pub name: String,
    pub platform: String,
    pub sync_enabled: bool,
    pub authenticate: bool,
}

#[derive(Clone, Debug)]
pub struct AccountAuthPrompt {
    pub account_id: u32,
    pub account_name: String,
    pub login_url: String,
}

impl PartialEq for AccountAuthPrompt {
    fn eq(&self, other: &Self) -> bool {
        self.account_id == other.account_id
            && self.account_name == other.account_name
            && self.login_url == other.login_url
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OverviewConfigFormData {
    pub auto_upload_interval_minutes: String,
    pub auto_upload_jitter_minutes: String,
    pub upload_on_launch: bool,
    pub no_upload_while_connected: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct UploadDestinationSummary {
    pub name: String,
    pub url: String,
    pub upload_enabled: bool,
    pub automatic: bool,
    pub auth: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryRow {
    pub account: String,
    pub match_id: String,
    pub timestamp: String,
    pub map_name: String,
    pub playlist: String,
    pub score: String,
    pub upload_destinations: Vec<HistoryUploadDestination>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryUploadDestination {
    pub target_name: String,
    pub state: String,
    pub uploaded: bool,
    pub upload_enabled: bool,
    pub location: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplayUploadRequest {
    pub target_name: String,
    pub match_id: String,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SyncRunState {
    pub running: bool,
    pub last_started_at: Option<String>,
    pub last_completed_at: Option<String>,
    pub last_summary: Option<BackfillSummary>,
    pub last_error: Option<String>,
}

impl SyncRunState {
    pub fn started(&self, started_at: String) -> Self {
        Self {
            running: true,
            last_started_at: Some(started_at),
            last_completed_at: self.last_completed_at.clone(),
            last_summary: self.last_summary.clone(),
            last_error: None,
        }
    }

    pub fn completed(&self, completed_at: String, summary: BackfillSummary) -> Self {
        Self {
            running: false,
            last_started_at: self.last_started_at.clone(),
            last_completed_at: Some(completed_at),
            last_summary: Some(summary),
            last_error: None,
        }
    }

    pub fn failed(&self, completed_at: String, error: String) -> Self {
        Self {
            running: false,
            last_started_at: self.last_started_at.clone(),
            last_completed_at: Some(completed_at),
            last_summary: self.last_summary.clone(),
            last_error: Some(error),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackfillSummary {
    pub uploaded: usize,
    pub duplicates: usize,
    pub cached: usize,
    pub failed: usize,
    pub failed_match_ids: Vec<String>,
    pub failed_uploads: Vec<ReplayUploadRequest>,
}

impl From<crate::sync::SyncSummary> for BackfillSummary {
    fn from(summary: crate::sync::SyncSummary) -> Self {
        Self {
            uploaded: summary.uploaded,
            duplicates: summary.duplicates,
            cached: summary.cached,
            failed: summary.failed,
            failed_match_ids: summary.failed_match_ids,
            failed_uploads: summary
                .failed_uploads
                .into_iter()
                .map(|failed| ReplayUploadRequest {
                    target_name: failed.target_name,
                    match_id: failed.match_id,
                    reason: Some(failed.reason),
                })
                .collect(),
        }
    }
}
