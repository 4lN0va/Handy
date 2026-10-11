//! Versioned transcript schema and its validator.
//!
//! The schema document itself lives at `schema/transcript-v1.0.json` in the
//! repository root and is embedded here with [`include_str!`], so the published
//! schema and the compiled validator can never drift apart.
//!
//! # Why a hand-written validator
//!
//! A full JSON-Schema engine is a large dependency to carry inside a desktop
//! binary for one small, closed schema. The validator below checks exactly the
//! constraints this document declares, and a test asserts that the embedded
//! schema and the validator's rules describe the same shape. What it is NOT is
//! a general JSON-Schema implementation — if the schema grows into one, this
//! should be replaced with a real engine rather than extended indefinitely.

use crate::error::VoiceApiError;
use crate::time::parse_rfc3339_utc;
use crate::transcript::{SegmentKind, Transcript, SCHEMA_VERSION};
use serde_json::Value;

/// The embedded JSON Schema for transcript v1.0.
pub const TRANSCRIPT_SCHEMA_V1: &str = include_str!("../../../schema/transcript-v1.0.json");

/// Schema versions this build can read.
pub const SUPPORTED_SCHEMA_VERSIONS: &[&str] = &["1.0"];

/// Validate a transcript value against the v1.0 schema.
///
/// Returns [`VoiceApiError::Schema`] with a precise, user-safe reason on
/// failure, or [`VoiceApiError::UnsupportedSchemaVersion`] when the document
/// declares a version this build does not implement.
pub fn validate_transcript(value: &Value) -> Result<(), VoiceApiError> {
    let root = value
        .as_object()
        .ok_or_else(|| VoiceApiError::Schema("root must be a JSON object".into()))?;

    // --- schema version ---
    // Accept either spelling; `schemaversion` is canonical.
    let version = root
        .get("schemaversion")
        .or_else(|| root.get("schema_version"))
        .ok_or_else(|| VoiceApiError::Schema("missing required field 'schemaversion'".into()))?
        .as_str()
        .ok_or_else(|| VoiceApiError::Schema("'schemaversion' must be a string".into()))?;
    if !SUPPORTED_SCHEMA_VERSIONS.contains(&version) {
        return Err(VoiceApiError::UnsupportedSchemaVersion {
            found: version.to_string(),
            supported: SCHEMA_VERSION,
        });
    }

    // --- session id ---
    let session_id = root
        .get("sessionid")
        .or_else(|| root.get("session_id"))
        .ok_or_else(|| VoiceApiError::Schema("missing required field 'sessionid'".into()))?
        .as_str()
        .ok_or_else(|| VoiceApiError::Schema("'sessionid' must be a string".into()))?;
    if session_id.trim().is_empty() {
        return Err(VoiceApiError::Schema(
            "'sessionid' must not be empty".into(),
        ));
    }

    // --- timestamps ---
    // `created_at` is required. `updated_at` is optional on read (an
    // interoperating producer may omit it) but must be well-formed when present.
    // This crate's exporter always writes both.
    let created_raw = root
        .get("created_at")
        .ok_or_else(|| VoiceApiError::Schema("missing required field 'created_at'".into()))?
        .as_str()
        .ok_or_else(|| VoiceApiError::Schema("'created_at' must be a string".into()))?;
    if parse_rfc3339_utc(created_raw).is_none() {
        return Err(VoiceApiError::Schema(format!(
            "'created_at' must be an RFC 3339 UTC timestamp like 2026-10-10T13:00:00Z, got '{created_raw}'"
        )));
    }
    if let Some(raw) = root.get("updated_at") {
        if !raw.is_null() {
            let raw = raw
                .as_str()
                .ok_or_else(|| VoiceApiError::Schema("'updated_at' must be a string".into()))?;
            if parse_rfc3339_utc(raw).is_none() {
                return Err(VoiceApiError::Schema(format!(
                    "'updated_at' must be an RFC 3339 UTC timestamp like 2026-10-10T13:04:12Z, got '{raw}'"
                )));
            }
        }
    }

    // --- language / source ---
    for field in ["language", "source"] {
        let raw = root
            .get(field)
            .ok_or_else(|| VoiceApiError::Schema(format!("missing required field '{field}'")))?
            .as_str()
            .ok_or_else(|| VoiceApiError::Schema(format!("'{field}' must be a string")))?;
        if raw.trim().is_empty() {
            return Err(VoiceApiError::Schema(format!(
                "'{field}' must not be empty"
            )));
        }
    }

    // --- engine (optional) ---
    if let Some(engine) = root.get("engine") {
        if !engine.is_null() {
            let engine = engine
                .as_object()
                .ok_or_else(|| VoiceApiError::Schema("'engine' must be an object".into()))?;
            match engine.get("id").and_then(Value::as_str) {
                Some(id) if !id.trim().is_empty() => {}
                _ => {
                    return Err(VoiceApiError::Schema(
                        "'engine.id' must be a non-empty string".into(),
                    ))
                }
            }
        }
    }

    // --- segments ---
    let segments = root
        .get("segments")
        .ok_or_else(|| VoiceApiError::Schema("missing required field 'segments'".into()))?
        .as_array()
        .ok_or_else(|| VoiceApiError::Schema("'segments' must be an array".into()))?;

    let mut seen_ids = std::collections::HashSet::new();
    let mut seen_segment_ids = std::collections::HashSet::new();

    for (i, raw) in segments.iter().enumerate() {
        let seg = raw
            .as_object()
            .ok_or_else(|| VoiceApiError::Schema(format!("segments[{i}] must be an object")))?;

        // id: integer >= 1, unique
        let id = seg
            .get("id")
            .ok_or_else(|| VoiceApiError::Schema(format!("segments[{i}] missing 'id'")))?
            .as_i64()
            .ok_or_else(|| VoiceApiError::Schema(format!("segments[{i}].id must be an integer")))?;
        if id < 1 {
            return Err(VoiceApiError::Schema(format!(
                "segments[{i}].id must be >= 1, got {id}"
            )));
        }
        if !seen_ids.insert(id) {
            return Err(VoiceApiError::Schema(format!(
                "segments[{i}].id {id} is duplicated"
            )));
        }

        // segment_id: optional on read. When absent, the display id is used as
        // the identity. When present it must be non-empty and unique.
        if let Some(raw_sid) = seg.get("segment_id") {
            let sid = raw_sid.as_str().ok_or_else(|| {
                VoiceApiError::Schema(format!("segments[{i}].segment_id must be a string"))
            })?;
            if sid.trim().is_empty() {
                return Err(VoiceApiError::Schema(format!(
                    "segments[{i}].segment_id must not be empty"
                )));
            }
            if !seen_segment_ids.insert(sid.to_string()) {
                return Err(VoiceApiError::Schema(format!(
                    "segments[{i}].segment_id '{sid}' is duplicated"
                )));
            }
        }

        // revision: optional on read; must be >= 1 when present.
        if let Some(raw_rev) = seg.get("revision") {
            let revision = raw_rev.as_i64().ok_or_else(|| {
                VoiceApiError::Schema(format!("segments[{i}].revision must be an integer"))
            })?;
            if revision < 1 {
                return Err(VoiceApiError::Schema(format!(
                    "segments[{i}].revision must be >= 1, got {revision}"
                )));
            }
        }

        // text: string (may be empty after a user edit)
        if seg.get("text").and_then(Value::as_str).is_none() {
            return Err(VoiceApiError::Schema(format!(
                "segments[{i}].text must be a string"
            )));
        }

        // final: boolean
        let is_final = seg.get("final").and_then(Value::as_bool).ok_or_else(|| {
            VoiceApiError::Schema(format!("segments[{i}].final must be a boolean"))
        })?;

        // kind: optional on read; when present it must be a known variant and
        // must agree with `final`.
        if let Some(raw_kind) = seg.get("kind") {
            let kind_raw = raw_kind.as_str().ok_or_else(|| {
                VoiceApiError::Schema(format!("segments[{i}].kind must be a string"))
            })?;
            let kind = match kind_raw {
                "partial" => SegmentKind::Partial,
                "final" => SegmentKind::Final,
                "error" => SegmentKind::Error,
                other => {
                    return Err(VoiceApiError::Schema(format!(
                        "segments[{i}].kind must be one of partial|final|error, got '{other}'"
                    )))
                }
            };
            if is_final != kind.is_final() {
                return Err(VoiceApiError::Schema(format!(
                    "segments[{i}]: 'final' is {is_final} but 'kind' is '{kind_raw}' — they must agree"
                )));
            }
        }

        // Optional timestamps: null or integer, and ordered when both present.
        let start = optional_int(seg.get("start_ms"), i, "start_ms")?;
        let end = optional_int(seg.get("end_ms"), i, "end_ms")?;
        if let (Some(s), Some(e)) = (start, end) {
            if e < s {
                return Err(VoiceApiError::Schema(format!(
                    "segments[{i}].end_ms ({e}) must not be before start_ms ({s})"
                )));
            }
        }
    }

    // --- errors (optional) ---
    if let Some(errors) = root.get("errors") {
        if !errors.is_null() {
            let errors = errors
                .as_array()
                .ok_or_else(|| VoiceApiError::Schema("'errors' must be an array".into()))?;
            for (i, raw) in errors.iter().enumerate() {
                let e = raw.as_object().ok_or_else(|| {
                    VoiceApiError::Schema(format!("errors[{i}] must be an object"))
                })?;
                match e.get("message").and_then(Value::as_str) {
                    Some(m) if !m.trim().is_empty() => {}
                    _ => {
                        return Err(VoiceApiError::Schema(format!(
                            "errors[{i}].message must be a non-empty string"
                        )))
                    }
                }
                let at = e.get("at").and_then(Value::as_str).ok_or_else(|| {
                    VoiceApiError::Schema(format!("errors[{i}].at must be a string"))
                })?;
                if parse_rfc3339_utc(at).is_none() {
                    return Err(VoiceApiError::Schema(format!(
                        "errors[{i}].at must be an RFC 3339 UTC timestamp, got '{at}'"
                    )));
                }
            }
        }
    }

    Ok(())
}

fn optional_int(
    value: Option<&Value>,
    index: usize,
    field: &str,
) -> Result<Option<i64>, VoiceApiError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_i64().map(Some).ok_or_else(|| {
            VoiceApiError::Schema(format!(
                "segments[{index}].{field} must be an integer or null"
            ))
        }),
    }
}

/// Validate a JSON string against the v1.0 schema.
pub fn validate_transcript_str(json: &str) -> Result<(), VoiceApiError> {
    let value: Value = serde_json::from_str(json)?;
    validate_transcript(&value)
}

/// Validate a typed transcript by serializing and checking the wire form.
///
/// Validating the *serialized* form (rather than the struct) is deliberate: the
/// wire document is the contract, and this catches a serde attribute that
/// renames or drops a required field.
pub fn validate_typed(transcript: &Transcript) -> Result<(), VoiceApiError> {
    let value = serde_json::to_value(transcript)?;
    validate_transcript(&value)
}

/// Parse the embedded schema document. Used by tests and tooling.
pub fn embedded_schema() -> Value {
    serde_json::from_str(TRANSCRIPT_SCHEMA_V1)
        .expect("embedded transcript schema must be valid JSON")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::FixedClock;
    use crate::transcript::{TranscriptManager, TranscriptUpdate};
    use serde_json::json;

    fn valid_document() -> Value {
        json!({
            "schemaversion": "1.0",
            "sessionid": "session-001",
            "created_at": "2026-10-10T13:00:00Z",
            "updated_at": "2026-10-10T13:04:12Z",
            "language": "zh-TW",
            "source": "microphone",
            "segments": [
                { "id": 1, "segment_id": "s1", "revision": 2, "text": "你好，我想測試語音辨識。", "final": true, "kind": "final" }
            ]
        })
    }

    #[test]
    fn embedded_schema_is_valid_json_and_declares_the_expected_version() {
        let schema = embedded_schema();
        assert_eq!(
            schema["properties"]["schemaversion"]["const"], "1.0",
            "schema must pin the version it describes"
        );
        assert!(schema["required"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "segments"));
    }

    #[test]
    fn specification_example_validates() {
        // The literal document from the product specification must be accepted.
        // Read-leniency matters: a hand-written or third-party document that
        // omits updated_at/revision/kind is still a valid v1.0 transcript.
        let doc = json!({
            "schemaversion": "1.0",
            "sessionid": "session-001",
            "created_at": "2026-10-10T13:00:00Z",
            "language": "zh-TW",
            "source": "microphone",
            "segments": [ { "id": 1, "text": "你好，我想測試語音辨識。", "final": true } ]
        });
        validate_transcript(&doc).expect("the specification's example must validate");
    }

    #[test]
    fn the_exporter_writes_the_full_field_set_even_though_reads_are_lenient() {
        // Read-leniency must not become write-sloppiness: our own export carries
        // every field, so downstream consumers never have to guess.
        let mut m = TranscriptManager::new(
            Box::new(FixedClock::reference()),
            "session-001",
            "zh-TW",
            "microphone",
        );
        m.apply(TranscriptUpdate::final_("s1", 1, "你好"));

        let value = serde_json::to_value(m.transcript()).unwrap();
        assert!(
            value.get("updated_at").is_some(),
            "export must include updated_at"
        );
        let seg = &value["segments"][0];
        assert!(
            seg.get("segment_id").is_some(),
            "export must include segment_id"
        );
        assert!(
            seg.get("revision").is_some(),
            "export must include revision"
        );
        assert!(seg.get("kind").is_some(), "export must include kind");
        assert_eq!(seg["final"], json!(true));
        assert_eq!(seg["kind"], json!("final"));
    }

    #[test]
    fn each_required_top_level_field_is_enforced() {
        // Driven by the schema's own `required` list so the document stays the
        // single source of truth.
        let required: Vec<String> = embedded_schema()["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        assert!(!required.is_empty());
        for missing in required {
            let mut doc = valid_document();
            doc.as_object_mut().unwrap().remove(&missing);
            let err = validate_transcript(&doc).unwrap_err();
            assert!(
                matches!(err, VoiceApiError::Schema(_)),
                "removing '{missing}' should fail schema validation"
            );
            assert!(
                err.to_string().contains(&missing),
                "error for '{missing}' should name the field, got: {err}"
            );
        }
    }

    #[test]
    fn a_malformed_updated_at_is_rejected_when_present() {
        let mut doc = valid_document();
        doc["updated_at"] = json!("yesterday");
        assert!(validate_transcript(&doc)
            .unwrap_err()
            .to_string()
            .contains("updated_at"));
    }

    #[test]
    fn a_well_formed_document_validates() {
        validate_transcript(&valid_document()).expect("should be valid");
    }

    #[test]
    fn a_manager_produced_transcript_always_validates() {
        let mut m = TranscriptManager::new(
            Box::new(FixedClock::reference()),
            "session-001",
            "zh-TW",
            "microphone",
        );
        m.apply(TranscriptUpdate::partial("s1", 1, "你好"));
        m.apply(TranscriptUpdate::final_(
            "s1",
            2,
            "你好，我想測試語音辨識。",
        ));
        m.record_error("裝置暫時中斷", None);
        validate_typed(m.transcript()).expect("manager output must satisfy the schema");
    }

    #[test]
    fn an_empty_transcript_is_valid() {
        // A session with no speech is legal; the UI shows an empty state.
        let m = TranscriptManager::new(
            Box::new(FixedClock::reference()),
            "empty-session",
            "zh-TW",
            "microphone",
        );
        validate_typed(m.transcript()).expect("empty transcript is valid");
    }

    #[test]
    fn rejects_a_non_object_root() {
        let err = validate_transcript(&json!([1, 2, 3])).unwrap_err();
        assert!(err.to_string().contains("root must be a JSON object"));
    }

    #[test]
    fn rejects_an_unsupported_schema_version_distinctly() {
        let mut doc = valid_document();
        doc["schemaversion"] = json!("2.0");
        let err = validate_transcript(&doc).unwrap_err();
        assert!(
            matches!(err, VoiceApiError::UnsupportedSchemaVersion { .. }),
            "a future version must be reported as unsupported, not malformed: {err}"
        );
    }

    #[test]
    fn accepts_the_snake_case_aliases() {
        let doc = json!({
            "schema_version": "1.0",
            "session_id": "session-001",
            "created_at": "2026-10-10T13:00:00Z",
            "updated_at": "2026-10-10T13:00:00Z",
            "language": "zh-TW",
            "source": "microphone",
            "segments": []
        });
        validate_transcript(&doc).expect("aliases should be accepted");
    }

    #[test]
    fn rejects_a_malformed_timestamp() {
        let mut doc = valid_document();
        doc["created_at"] = json!("10/10/2026 13:00");
        let err = validate_transcript(&doc).unwrap_err();
        assert!(err.to_string().contains("created_at"));
    }

    #[test]
    fn rejects_empty_language_or_source() {
        for field in ["language", "source"] {
            let mut doc = valid_document();
            doc[field] = json!("  ");
            let err = validate_transcript(&doc).unwrap_err();
            assert!(err.to_string().contains(field), "got: {err}");
        }
    }

    #[test]
    fn rejects_duplicate_segment_ids() {
        let mut doc = valid_document();
        doc["segments"] = json!([
            { "id": 1, "segment_id": "dup", "revision": 1, "text": "a", "final": true, "kind": "final" },
            { "id": 2, "segment_id": "dup", "revision": 1, "text": "b", "final": true, "kind": "final" }
        ]);
        let err = validate_transcript(&doc).unwrap_err();
        assert!(err.to_string().contains("duplicated"), "got: {err}");
    }

    #[test]
    fn rejects_duplicate_display_ids() {
        let mut doc = valid_document();
        doc["segments"] = json!([
            { "id": 1, "segment_id": "a", "revision": 1, "text": "a", "final": true, "kind": "final" },
            { "id": 1, "segment_id": "b", "revision": 1, "text": "b", "final": true, "kind": "final" }
        ]);
        assert!(validate_transcript(&doc)
            .unwrap_err()
            .to_string()
            .contains("duplicated"));
    }

    #[test]
    fn rejects_a_zero_or_negative_id_and_revision() {
        let mut doc = valid_document();
        doc["segments"][0]["id"] = json!(0);
        assert!(validate_transcript(&doc)
            .unwrap_err()
            .to_string()
            .contains(">= 1"));

        let mut doc = valid_document();
        doc["segments"][0]["revision"] = json!(0);
        assert!(validate_transcript(&doc)
            .unwrap_err()
            .to_string()
            .contains(">= 1"));
    }

    #[test]
    fn rejects_final_and_kind_disagreeing() {
        let mut doc = valid_document();
        doc["segments"][0]["final"] = json!(false);
        let err = validate_transcript(&doc).unwrap_err();
        assert!(err.to_string().contains("must agree"), "got: {err}");
    }

    #[test]
    fn rejects_an_unknown_kind() {
        let mut doc = valid_document();
        doc["segments"][0]["kind"] = json!("guessing");
        let err = validate_transcript(&doc).unwrap_err();
        assert!(err.to_string().contains("partial|final|error"));
    }

    #[test]
    fn rejects_end_before_start() {
        let mut doc = valid_document();
        doc["segments"][0]["start_ms"] = json!(2000);
        doc["segments"][0]["end_ms"] = json!(1000);
        assert!(validate_transcript(&doc)
            .unwrap_err()
            .to_string()
            .contains("before"));
    }

    #[test]
    fn accepts_null_and_absent_timestamps() {
        let mut doc = valid_document();
        doc["segments"][0]["start_ms"] = json!(null);
        doc["segments"][0]["end_ms"] = json!(null);
        validate_transcript(&doc).expect("explicit nulls are allowed");

        let mut doc = valid_document();
        doc["segments"][0]
            .as_object_mut()
            .unwrap()
            .remove("start_ms");
        validate_transcript(&doc).expect("absent timestamps are allowed");
    }

    #[test]
    fn accepts_a_tentative_segment() {
        let mut doc = valid_document();
        doc["segments"] = json!([
            { "id": 1, "segment_id": "s1", "revision": 3, "text": "還在辨識", "final": false, "kind": "partial" }
        ]);
        validate_transcript(&doc).expect("partial segments are valid documents");
    }

    #[test]
    fn accepts_and_validates_the_errors_array() {
        let mut doc = valid_document();
        doc["errors"] = json!([{ "message": "麥克風中斷", "at": "2026-10-10T13:01:00Z" }]);
        validate_transcript(&doc).expect("errors array is valid");

        doc["errors"] = json!([{ "message": "", "at": "2026-10-10T13:01:00Z" }]);
        assert!(validate_transcript(&doc)
            .unwrap_err()
            .to_string()
            .contains("message"));

        doc["errors"] = json!([{ "message": "ok", "at": "not-a-time" }]);
        assert!(validate_transcript(&doc)
            .unwrap_err()
            .to_string()
            .contains("at"));
    }

    #[test]
    fn validate_str_reports_invalid_json() {
        let err = validate_transcript_str("{ not json").unwrap_err();
        assert!(matches!(err, VoiceApiError::Serialize(_)));
    }

    #[test]
    fn validator_rules_cover_every_required_field_the_schema_declares() {
        // Guards against the schema document and the hand-written validator
        // drifting apart: every field the schema marks required must be
        // independently required by the validator.
        let schema = embedded_schema();
        let required: Vec<String> = schema["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        for field in required {
            let mut doc = valid_document();
            doc.as_object_mut().unwrap().remove(&field);
            assert!(
                validate_transcript(&doc).is_err(),
                "schema requires '{field}' but the validator accepted its absence"
            );
        }
    }

    #[test]
    fn validator_requires_every_segment_field_the_schema_declares() {
        let schema = embedded_schema();
        let required: Vec<String> = schema["properties"]["segments"]["items"]["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        for field in required {
            let mut doc = valid_document();
            doc["segments"][0].as_object_mut().unwrap().remove(&field);
            assert!(
                validate_transcript(&doc).is_err(),
                "schema requires segment field '{field}' but the validator accepted its absence"
            );
        }
    }
}
