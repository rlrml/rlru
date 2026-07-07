use super::*;
use psynet::{SaveTrainingData, SaveTrainingRound};

fn sample_payload() -> SaveTrainingData {
    SaveTrainingData {
        tm_guid: String::new(),
        tm_name: "My Pack".to_string(),
        training_type: 3,
        difficulty: 1,
        map_name: "Park_P".to_string(),
        tags: vec![],
        num_rounds: 0,
        rounds: vec![SaveTrainingRound {
            time_limit: 8.0,
            serialized_archetypes: vec!["{\"a\":1}".to_string()],
        }],
        description: Some("desc".to_string()),
    }
}

#[test]
fn deserializes_pascal_case_payload_with_optional_fields_omitted() {
    let json = r#"{
        "TM_Name": "JSON Pack",
        "Type": 3,
        "Difficulty": 1,
        "MapName": "Park_P",
        "Rounds": [
            {"TimeLimit": 8.0, "SerializedArchetypes": ["{\"a\":1}"]}
        ]
    }"#;
    let payload: SaveTrainingData = serde_json::from_str(json).expect("parses");
    assert_eq!(payload.tm_name, "JSON Pack");
    assert_eq!(payload.tm_guid, "");
    assert_eq!(payload.tags, Vec::<String>::new());
    assert_eq!(payload.num_rounds, 0);
    assert_eq!(payload.rounds.len(), 1);
    assert_eq!(payload.description, None);
}

#[test]
fn finalize_mints_guid_and_recomputes_num_rounds() {
    let finalized = finalize_payload(sample_payload(), None).expect("finalizes");
    assert_eq!(finalized.num_rounds, 1);
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(&finalized.tm_guid)
        .expect("valid base64 guid");
    assert_eq!(decoded.len(), 16);
}

#[test]
fn finalize_preserves_valid_supplied_guid() {
    let mut payload = sample_payload();
    let guid = SaveTrainingData::new_guid();
    payload.tm_guid = guid.clone();
    let finalized = finalize_payload(payload, None).expect("finalizes");
    assert_eq!(finalized.tm_guid, guid);
}

#[test]
fn finalize_rejects_malformed_guid() {
    let mut payload = sample_payload();
    payload.tm_guid = "not-base64!!".to_string();
    assert!(finalize_payload(payload, None).is_err());

    let mut payload = sample_payload();
    payload.tm_guid = base64::engine::general_purpose::STANDARD.encode([0u8; 8]);
    assert!(finalize_payload(payload, None).is_err());
}

#[test]
fn name_override_takes_precedence() {
    let finalized = finalize_payload(sample_payload(), Some("renamed")).expect("finalizes");
    assert_eq!(finalized.tm_name, "renamed");
}

#[test]
fn finalize_rejects_empty_name_rounds_and_map() {
    let mut payload = sample_payload();
    payload.tm_name = "  ".to_string();
    assert!(finalize_payload(payload, None).is_err());

    let mut payload = sample_payload();
    payload.rounds.clear();
    assert!(finalize_payload(payload, None).is_err());

    let mut payload = sample_payload();
    payload.rounds[0].serialized_archetypes.clear();
    assert!(finalize_payload(payload, None).is_err());

    let mut payload = sample_payload();
    payload.map_name = String::new();
    assert!(finalize_payload(payload, None).is_err());
}
