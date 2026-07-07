//! Maps a decoded custom training pack (`.tem`) into a PsyNet publish request
//! and publishes it, returning the public share code.

use std::path::Path;

use anyhow::{Context, Result};
use psynet::{SaveTrainingData, SaveTrainingRound};
use subtr_actor_training::{Difficulty, TrainingFile, TrainingPack, TrainingType};

/// `ETrainingType` wire ordinal for a parsed [`TrainingType`].
fn training_type_ordinal(t: &TrainingType) -> i32 {
    match t {
        TrainingType::None => 0,
        TrainingType::Aerial => 1,
        TrainingType::Goalie => 2,
        TrainingType::Striker => 3,
        TrainingType::End => 4,
        // Unknown future variant: default to None rather than guess.
        TrainingType::Other(_) => 0,
    }
}

/// `EDifficulty` wire ordinal for a parsed [`Difficulty`].
fn difficulty_ordinal(d: &Difficulty) -> i32 {
    match d {
        Difficulty::Easy => 0,
        Difficulty::Medium => 1,
        Difficulty::Hard => 2,
        Difficulty::End => 3,
        Difficulty::Other(_) => 0,
    }
}

/// Builds a [`SaveTrainingData`] from a decoded [`TrainingPack`], minting a
/// fresh GUID so the upload is treated as a new pack. `name_override` replaces
/// the pack's stored name when supplied.
pub fn pack_to_save_request(pack: &TrainingPack, name_override: Option<&str>) -> SaveTrainingData {
    let rounds = pack
        .rounds
        .iter()
        .map(|r| SaveTrainingRound {
            time_limit: r.time_limit,
            serialized_archetypes: r.serialized_archetypes.clone(),
        })
        .collect::<Vec<_>>();

    let name = name_override
        .map(str::to_string)
        .or_else(|| pack.name.clone())
        .unwrap_or_else(|| "Untitled Training Pack".to_string());

    SaveTrainingData {
        tm_guid: SaveTrainingData::new_guid(),
        tm_name: name,
        training_type: training_type_ordinal(&pack.training_type),
        difficulty: difficulty_ordinal(&pack.difficulty),
        map_name: pack.map_name.clone().unwrap_or_default(),
        // PsyNet browse/get report Tags as a string array; local packs carry
        // integer tag ids which have no wire counterpart, so publish untagged.
        tags: Vec::new(),
        num_rounds: pack.rounds.len() as i32,
        rounds,
        description: pack.description.clone(),
    }
}

/// Decodes a `.tem` training pack file into a [`TrainingPack`].
pub fn decode_pack(path: &Path) -> Result<TrainingPack> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("failed to read training pack {}", path.display()))?;
    let file = TrainingFile::from_bytes(&bytes)
        .map_err(|e| anyhow::anyhow!("failed to parse training pack {}: {e}", path.display()))?;
    file.pack()
        .map_err(|e| anyhow::anyhow!("failed to extract typed pack from {}: {e}", path.display()))
}

#[cfg(test)]
#[path = "training_tests.rs"]
mod tests;
