//! Empirical discovery probe for the PsyNet Training write/publish service.
//!
//! Restores the selected account's saved session, sanity-checks a known read
//! (`Training/BrowseTrainingData v1`), then calls each candidate write-service
//! name with a minimal body and prints the resulting PsyError kind+message so
//! we can tell "service not found / unsupported" apart from a real validation
//! or permission error on an existing service.
//!
//! Run: `cargo run --example training_probe [-- <account-name>]`

use std::path::PathBuf;

use anyhow::{Context, Result};
use rlru::auth::AuthManager;
use rlru::config::Config;
use rlru::paths::AppPaths;
use secrecy::ExposeSecret;
use serde_json::json;

const LOG_DIR: &str = "/tmp/claude-1000/-home-imalison-Projects-subtr-actor/7fdd5b11-110c-4854-b86c-fd466b4353a2/scratchpad/psynet-training-probe";

#[tokio::main]
async fn main() -> Result<()> {
    let account_arg = std::env::args().nth(1);

    let paths = AppPaths::discover()?;
    let config_path = paths.config_file();
    let config = Config::load_or_default(&config_path)?;

    let account = match account_arg {
        Some(name) => config
            .accounts
            .iter()
            .find(|c| c.name == name)
            .with_context(|| format!("unknown account {name:?}"))?,
        None => {
            let selected = config
                .behavior
                .selected_account
                .as_deref()
                .context("no selected_account in config")?;
            config
                .accounts
                .iter()
                .find(|c| c.name == selected)
                .with_context(|| format!("selected account {selected:?} not found"))?
        }
    };
    eprintln!("using account {} (id {})", account.name, account.id);

    let auth = AuthManager::for_account(&paths, account);
    let eos = auth
        .restore_or_refresh()
        .await
        .context("failed to restore/refresh session")?;
    eprintln!("restored EOS session for account_id {}", eos.account_id);

    let client = psynet::PsyNetClient::new();
    let account_id = eos.account_id.clone();
    let access_token = eos.access_token.expose_secret().to_string();

    let mut rpc: psynet::PsyNetRpc = client
        .auth_player(&account_id, &access_token)
        .await
        .context("auth_player failed")?;
    eprintln!("authenticated PsyNet websocket session");

    let mut log = String::new();
    macro_rules! record {
        ($($arg:tt)*) => {{
            let line = format!($($arg)*);
            println!("{line}");
            log.push_str(&line);
            log.push('\n');
        }};
    }

    // Call a service, transparently reconnecting the socket if a transport
    // error (e.g. a prior request closed/broke the socket) occurs. Returns the
    // Ok/PsyError result once we have a live socket, or a transport error
    // string if reconnection also fails.
    async fn call_resilient(
        client: &psynet::PsyNetClient,
        rpc: &mut psynet::PsyNetRpc,
        account_id: &str,
        access_token: &str,
        service: &str,
        body: &serde_json::Value,
    ) -> std::result::Result<std::result::Result<String, psynet::PsyError>, String> {
        match rpc.call_raw(service, body).await {
            Ok(v) => Ok(v),
            Err(first) => {
                // reconnect and retry once
                match client.auth_player(account_id, access_token).await {
                    Ok(fresh) => {
                        *rpc = fresh;
                        rpc.call_raw(service, body)
                            .await
                            .map_err(|e| format!("{first:#} -> after reconnect: {e:#}"))
                    }
                    Err(e) => Err(format!("{first:#} -> reconnect failed: {e:#}")),
                }
            }
        }
    }

    // --- Sanity: known read proves auth works ---
    record!("=== SANITY READ: Training/BrowseTrainingData v1 ===");
    match call_resilient(
        &client,
        &mut rpc,
        &account_id,
        &access_token,
        "Training/BrowseTrainingData v1",
        &json!({ "bFeaturedOnly": true }),
    )
    .await
    {
        Ok(Ok(result)) => record!("OK ({} bytes): {}", result.len(), truncate(&result, 600)),
        Ok(Err(e)) => record!("PsyError kind={:?} message={:?}", e.kind, e.message),
        Err(e) => record!("TRANSPORT ERROR: {e}"),
    }

    record!("\n=== READ: Training/GetTrainingData v1 (oracle: real service) ===");
    match call_resilient(
        &client,
        &mut rpc,
        &account_id,
        &access_token,
        "Training/GetTrainingData v1",
        &json!({ "Code": "" }),
    )
    .await
    {
        Ok(Ok(result)) => record!("OK ({} bytes): {}", result.len(), truncate(&result, 400)),
        Ok(Err(e)) => record!("PsyError kind={:?} message={:?}", e.kind, e.message),
        Err(e) => record!("TRANSPORT ERROR: {e}"),
    }

    // --- Candidate write services (version-suffixed only; bare names hang the
    // socket and yield no oracle signal) ---
    let verbs = [
        "Save", "Upload", "Publish", "Create", "Set", "Add", "Submit", "Update", "Put", "Post",
        "Store", "Register",
    ];
    let mut candidates: Vec<String> = Vec::new();
    for verb in verbs {
        for ver in ["v1", "v2"] {
            candidates.push(format!("Training/{verb}TrainingData {ver}"));
        }
    }
    // Plausible mint/code and alternative-noun services.
    for extra in [
        "Codes/CreateTrainingCode v1",
        "Training/SaveTrainingCode v1",
        "Training/CreateTrainingCode v1",
        "Training/PublishTrainingCode v1",
        "Training/SaveTraining v1",
        "Training/UploadTraining v1",
        "Training/CreateTraining v1",
        "Training/SaveTrainingMetadata v1",
        "Training/SetTrainingMetadata v1",
    ] {
        candidates.push(extra.to_string());
    }

    record!("\n=== WRITE-SERVICE PROBES (minimal empty body) ===");
    for service in &candidates {
        match call_resilient(
            &client,
            &mut rpc,
            &account_id,
            &access_token,
            service,
            &json!({}),
        )
        .await
        {
            Ok(Ok(result)) => {
                record!("[{service}] UNEXPECTED OK: {}", truncate(&result, 300))
            }
            Ok(Err(e)) => record!("[{service}] kind={:?} msg={:?}", e.kind, e.message),
            Err(e) => record!("[{service}] TRANSPORT ERROR: {e}"),
        }
    }

    let log_path = PathBuf::from(LOG_DIR).join("probe-run.log");
    std::fs::write(&log_path, &log).ok();
    eprintln!("\nwrote log to {}", log_path.display());

    rpc.close().await.ok();
    Ok(())
}

fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n {
        s.to_string()
    } else {
        format!("{}… (+{} bytes)", &s[..n], s.len() - n)
    }
}
