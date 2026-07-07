use super::*;
use subtr_actor_training::{Difficulty, Guid, PlayerId, Round, TrainingPack, TrainingType};

fn sample_pack() -> TrainingPack {
    TrainingPack {
        guid: Guid::default(),
        code: None,
        name: Some("My Pack".to_string()),
        training_type: TrainingType::Striker,
        difficulty: Difficulty::Medium,
        creator_name: None,
        description: Some("desc".to_string()),
        tags: vec![],
        map_name: Some("Park_P".to_string()),
        created_at: 0,
        updated_at: 0,
        creator_player_id: PlayerId::default(),
        rounds: vec![Round {
            time_limit: 8.0,
            serialized_archetypes: vec!["{\"a\":1}".to_string()],
        }],
        player_team_number: 0,
        unowned: false,
        perfect_completed: false,
        shots_completed: 0,
    }
}

#[test]
fn maps_core_fields_and_enums() {
    let pack = sample_pack();
    let req = pack_to_save_request(&pack, None);

    assert_eq!(req.tm_name, "My Pack");
    assert_eq!(req.training_type, 3); // Striker
    assert_eq!(req.difficulty, 1); // Medium
    assert_eq!(req.map_name, "Park_P");
    assert_eq!(req.num_rounds, 1);
    assert_eq!(req.rounds.len(), 1);
    assert_eq!(req.rounds[0].time_limit, 8.0);
    assert_eq!(req.description.as_deref(), Some("desc"));
}

#[test]
fn name_override_takes_precedence() {
    let pack = sample_pack();
    let req = pack_to_save_request(&pack, Some("renamed"));
    assert_eq!(req.tm_name, "renamed");
}

#[test]
fn generates_distinct_valid_base64_guids() {
    use base64::Engine;
    let a = SaveTrainingData::new_guid();
    let b = SaveTrainingData::new_guid();
    assert_ne!(a, b);
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(&a)
        .expect("valid base64 guid");
    assert_eq!(decoded.len(), 16);
}
