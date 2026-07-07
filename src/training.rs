//! Loads a structured training-pack payload (JSON matching
//! [`psynet::SaveTrainingData`]'s PascalCase wire schema), validates and
//! finalizes it for publishing to PsyNet.
//!
//! rlru deliberately does not parse the game's encrypted `.tem` files; a
//! payload file is produced externally (e.g. by the subtr-actor stats player
//! or a `.tem`-to-payload exporter) and published here verbatim.

use std::path::Path;

use anyhow::{bail, Context, Result};
use base64::Engine;
use psynet::SaveTrainingData;

/// Reads and deserializes a structured training payload file.
pub fn load_payload(path: &Path) -> Result<SaveTrainingData> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("failed to read training payload {}", path.display()))?;
    serde_json::from_slice(&bytes)
        .with_context(|| format!("failed to parse training payload {}", path.display()))
}

/// Validates a loaded payload and fills in derivable fields so it is ready to
/// publish: applies `name_override`, mints a fresh `TM_Guid` when the payload
/// omits one, and recomputes `NumRounds` from the rounds list.
pub fn finalize_payload(
    mut payload: SaveTrainingData,
    name_override: Option<&str>,
) -> Result<SaveTrainingData> {
    if let Some(name) = name_override {
        payload.tm_name = name.to_string();
    }
    if payload.tm_name.trim().is_empty() {
        bail!("training payload must have a non-empty TM_Name (or pass --name)");
    }
    if payload.rounds.is_empty() {
        bail!("training payload must contain at least one round");
    }
    if payload
        .rounds
        .iter()
        .any(|r| r.serialized_archetypes.is_empty())
    {
        bail!("every round must contain at least one serialized archetype");
    }
    if payload.map_name.trim().is_empty() {
        bail!("training payload must have a non-empty MapName");
    }

    if payload.tm_guid.is_empty() {
        payload.tm_guid = SaveTrainingData::new_guid();
    } else {
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(&payload.tm_guid)
            .context("TM_Guid must be valid base64")?;
        if decoded.len() != 16 {
            bail!(
                "TM_Guid must decode to 16 bytes, got {} (omit it to mint a fresh GUID)",
                decoded.len()
            );
        }
    }

    payload.num_rounds = payload.rounds.len() as i32;
    Ok(payload)
}

#[cfg(test)]
#[path = "training_tests.rs"]
mod tests;
