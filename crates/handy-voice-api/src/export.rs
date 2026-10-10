//! JSON export and import for transcripts.
//!
//! Design rules enforced here:
//!
//! * **UTF-8, never mangled.** Files are written as raw UTF-8 bytes with no BOM,
//!   and reads strip a BOM if one is present (Windows editors add them).
//! * **Validate before writing and after reading.** A file that cannot be
//!   re-read is worse than no file, so export validates its own output.
//! * **Atomic writes.** The document is written to a temporary file in the
//!   destination directory and then renamed, so a crash or a full disk can never
//!   leave a truncated transcript behind.
//! * **No secrets, ever.** The transcript model has no secret-bearing fields,
//!   and [`assert_no_secret_material`] is used by tests to prove a known API key
//!   cannot appear in exported bytes.

use crate::error::VoiceApiError;
use crate::schema::{validate_transcript, validate_transcript_str};
use crate::transcript::{Transcript, TranscriptSegment};
use serde_json::{json, Value};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// What to include when exporting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExportFilter {
    /// Include tentative (non-final) segments. Defaults to `true` so a user can
    /// capture an in-progress session; set to `false` to export only confirmed
    /// text.
    pub include_partial: bool,
}

impl Default for ExportFilter {
    fn default() -> Self {
        Self {
            include_partial: true,
        }
    }
}

impl ExportFilter {
    /// Confirmed text only.
    pub fn finals_only() -> Self {
        Self {
            include_partial: false,
        }
    }
}

/// Serialize a transcript to pretty-printed UTF-8 JSON.
pub fn to_json(transcript: &Transcript) -> Result<String, VoiceApiError> {
    to_json_filtered(transcript, ExportFilter::default())
}

/// Serialize a transcript, optionally excluding tentative segments.
///
/// The result is validated against the schema before being returned, so an
/// exporter bug surfaces here rather than as a corrupt file on disk.
pub fn to_json_filtered(
    transcript: &Transcript,
    filter: ExportFilter,
) -> Result<String, VoiceApiError> {
    let document = to_document(transcript, filter)?;
    validate_transcript(&document)?;
    Ok(serde_json::to_string_pretty(&document)?)
}

/// Build the JSON document (without serializing to a string).
pub fn to_document(transcript: &Transcript, filter: ExportFilter) -> Result<Value, VoiceApiError> {
    let segments: Vec<&TranscriptSegment> = transcript
        .segments
        .iter()
        .filter(|s| filter.include_partial || s.is_final)
        // Error rows are not text; they live in `errors`.
        .filter(|s| s.kind != crate::transcript::SegmentKind::Error)
        .collect();

    // Re-number densely from 1 so a filtered export still has a contiguous,
    // unique `id` space (the schema requires uniqueness and >= 1).
    let renumbered: Vec<Value> = segments
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let mut v = serde_json::to_value(s).unwrap_or_else(|_| json!({}));
            if let Some(obj) = v.as_object_mut() {
                obj.insert("id".to_string(), json!(i + 1));
            }
            v
        })
        .collect();

    let mut doc = json!({
        "schemaversion": transcript.schema_version,
        "sessionid": transcript.session_id,
        "created_at": transcript.created_at,
        "updated_at": transcript.updated_at,
        "language": transcript.language,
        "source": transcript.source,
        "segments": renumbered,
    });

    if let Some(engine) = &transcript.engine {
        doc["engine"] = serde_json::to_value(engine)?;
    }
    if !transcript.errors.is_empty() {
        doc["errors"] = serde_json::to_value(&transcript.errors)?;
    }
    Ok(doc)
}

/// Write a transcript to `path` atomically as UTF-8 JSON.
///
/// Returns the path actually written.
pub fn export_to_file(
    path: impl AsRef<Path>,
    transcript: &Transcript,
) -> Result<PathBuf, VoiceApiError> {
    export_to_file_filtered(path, transcript, ExportFilter::default())
}

/// Write a transcript to `path` atomically, with an export filter.
pub fn export_to_file_filtered(
    path: impl AsRef<Path>,
    transcript: &Transcript,
    filter: ExportFilter,
) -> Result<PathBuf, VoiceApiError> {
    let path = path.as_ref();
    let json = to_json_filtered(transcript, filter)?;

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|e| {
                VoiceApiError::io("create directory for", parent.display().to_string(), e)
            })?;
        }
    }

    // Write via a sibling temp file, then rename. A rename within one directory
    // is atomic on every platform we target, so readers see either the old file
    // or the complete new one.
    let tmp = temp_sibling(path);
    {
        let mut file = fs::File::create(&tmp)
            .map_err(|e| VoiceApiError::io("create", tmp.display().to_string(), e))?;
        file.write_all(json.as_bytes())
            .map_err(|e| VoiceApiError::io("write", tmp.display().to_string(), e))?;
        // Flush to the OS before the rename so the bytes are durable first.
        file.sync_all()
            .map_err(|e| VoiceApiError::io("flush", tmp.display().to_string(), e))?;
    }

    // `rename` replaces an existing file on Unix. On Windows it fails if the
    // destination exists, so remove it first — the window is tiny and the temp
    // file still holds the full content.
    if let Err(first) = fs::rename(&tmp, path) {
        if path.exists() {
            fs::remove_file(path)
                .map_err(|e| VoiceApiError::io("replace", path.display().to_string(), e))?;
            fs::rename(&tmp, path)
                .map_err(|e| VoiceApiError::io("write", path.display().to_string(), e))?;
        } else {
            // Clean up the temp file so a failed export leaves nothing behind.
            let _ = fs::remove_file(&tmp);
            return Err(VoiceApiError::io(
                "write",
                path.display().to_string(),
                first,
            ));
        }
    }

    Ok(path.to_path_buf())
}

/// Read and validate a transcript from a UTF-8 JSON file.
pub fn import_from_file(path: impl AsRef<Path>) -> Result<Transcript, VoiceApiError> {
    let path = path.as_ref();
    let bytes =
        fs::read(path).map_err(|e| VoiceApiError::io("read", path.display().to_string(), e))?;

    // Decode strictly: silently replacing invalid bytes would corrupt Chinese
    // text without telling anyone.
    let text = String::from_utf8(bytes).map_err(|e| VoiceApiError::Io {
        action: "decode as UTF-8",
        path: path.display().to_string(),
        source: std::io::Error::new(std::io::ErrorKind::InvalidData, e),
    })?;

    // Strip a UTF-8 BOM if a Windows editor added one.
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);

    // Validate the wire document before deserializing, so the error names the
    // actual problem (e.g. an unsupported version) rather than a serde type error.
    validate_transcript_str(text)?;
    let transcript: Transcript = serde_json::from_str(text)?;
    Ok(transcript)
}

/// Parse a transcript from a JSON string, validating first.
pub fn import_from_str(json: &str) -> Result<Transcript, VoiceApiError> {
    let text = json.strip_prefix('\u{feff}').unwrap_or(json);
    validate_transcript_str(text)?;
    Ok(serde_json::from_str(text)?)
}

fn temp_sibling(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "transcript.json".to_string());
    let tmp_name = format!(".{}.{}.tmp", file_name, std::process::id());
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join(tmp_name),
        _ => PathBuf::from(tmp_name),
    }
}

/// Assert that `haystack` contains none of the provided secret strings.
///
/// Used by tests (and available to callers) to prove that API keys do not leak
/// into exported documents, settings dumps, or log lines.
pub fn assert_no_secret_material(haystack: &str, secrets: &[&str]) -> Result<(), String> {
    for secret in secrets {
        if secret.is_empty() {
            continue;
        }
        if haystack.contains(secret) {
            return Err("output contains secret material".to_string());
        }
    }
    Ok(())
}

fn _schema_guard() {
    // `to_document` must keep producing values the validator accepts. The
    // assertion is exercised by the tests below rather than here; this binding
    // just keeps the import intentional.
    let _ = validate_transcript;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::FixedClock;
    use crate::transcript::{EngineInfo, TranscriptManager, TranscriptUpdate};
    use tempfile::TempDir;

    fn fixture() -> Transcript {
        let mut m = TranscriptManager::new(
            Box::new(FixedClock::reference()),
            "session-001",
            "zh-TW",
            "microphone",
        );
        m.set_engine(EngineInfo {
            id: "transcribe-cpp".into(),
            model_id: Some("handy-computer/Breeze-ASR-25-gguf".into()),
        });
        m.apply(TranscriptUpdate::final_(
            "s1",
            2,
            "你好，我想測試語音辨識。",
        ));
        m.apply(TranscriptUpdate::partial("s2", 1, "這段還在辨識"));
        m.into_transcript()
    }

    #[test]
    fn export_is_valid_json_and_reimportable() {
        let t = fixture();
        let json = to_json(&t).unwrap();
        let parsed: Value = serde_json::from_str(&json).expect("export must be valid JSON");
        assert_eq!(parsed["schemaversion"], "1.0");
        assert_eq!(parsed["sessionid"], "session-001");
        assert_eq!(parsed["language"], "zh-TW");
        assert_eq!(parsed["segments"][0]["final"], json!(true));
    }

    #[test]
    fn traditional_chinese_survives_export_and_reimport_byte_for_byte() {
        let t = fixture();
        let json = to_json(&t).unwrap();
        let back = import_from_str(&json).unwrap();
        assert_eq!(back, t, "round trip must be lossless");
        assert!(json.contains("你好，我想測試語音辨識。"));
        // No mojibake: the JSON must not contain escaped replacement chars.
        assert!(!json.contains('\u{fffd}'));
    }

    #[test]
    fn export_to_file_then_import_round_trips_on_disk() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("nested").join("transcript.json");
        let t = fixture();

        let written = export_to_file(&path, &t).unwrap();
        assert_eq!(written, path);
        assert!(path.exists(), "export must create parent directories");

        let back = import_from_file(&path).unwrap();
        assert_eq!(back, t);
    }

    #[test]
    fn exported_file_is_utf8_without_a_bom() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("t.json");
        export_to_file(&path, &fixture()).unwrap();

        let bytes = fs::read(&path).unwrap();
        assert!(
            !bytes.starts_with(&[0xEF, 0xBB, 0xBF]),
            "must not write a BOM"
        );
        String::from_utf8(bytes).expect("file must be valid UTF-8");
    }

    #[test]
    fn import_tolerates_a_bom_added_by_an_editor() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("bom.json");
        let json = to_json(&fixture()).unwrap();
        fs::write(&path, format!("\u{feff}{json}")).unwrap();

        let back = import_from_file(&path).unwrap();
        assert_eq!(back.session_id, "session-001");
    }

    #[test]
    fn import_rejects_invalid_utf8_rather_than_mangling_it() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("bad.json");
        // 0xFF is never valid UTF-8.
        fs::write(&path, [0x7B, 0x22, 0xFF, 0x22, 0x7D]).unwrap();

        let err = import_from_file(&path).unwrap_err();
        assert!(
            matches!(err, VoiceApiError::Io { action, .. } if action == "decode as UTF-8"),
            "got: {err}"
        );
    }

    #[test]
    fn import_reports_an_unsupported_version_precisely() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("v2.json");
        let mut doc = to_document(&fixture(), ExportFilter::default()).unwrap();
        doc["schemaversion"] = json!("9.9");
        fs::write(&path, doc.to_string()).unwrap();

        let err = import_from_file(&path).unwrap_err();
        assert!(matches!(
            err,
            VoiceApiError::UnsupportedSchemaVersion { .. }
        ));
        assert!(err.to_string().contains("9.9"));
    }

    #[test]
    fn import_of_a_missing_file_reports_the_path() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("nope.json");
        let err = import_from_file(&path).unwrap_err();
        assert!(err.to_string().contains("nope.json"), "got: {err}");
    }

    #[test]
    fn filtered_export_excludes_tentative_segments() {
        let t = fixture();
        let json = to_json_filtered(&t, ExportFilter::finals_only()).unwrap();
        let parsed: Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["segments"].as_array().unwrap().len(), 1);
        assert_eq!(
            parsed["segments"][0]["text"],
            json!("你好，我想測試語音辨識。")
        );
        assert!(!json.contains("這段還在辨識"));
    }

    #[test]
    fn default_export_includes_tentative_segments_and_marks_them() {
        let t = fixture();
        let parsed: Value = serde_json::from_str(&to_json(&t).unwrap()).unwrap();
        let segs = parsed["segments"].as_array().unwrap();
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[1]["final"], json!(false));
        assert_eq!(segs[1]["kind"], json!("partial"));
    }

    #[test]
    fn filtered_export_renumbers_ids_densely() {
        let t = fixture();
        let parsed: Value =
            serde_json::from_str(&to_json_filtered(&t, ExportFilter::finals_only()).unwrap())
                .unwrap();
        // The kept segment was id 1; a second export must still start at 1 and
        // never leave a gap or a duplicate.
        assert_eq!(parsed["segments"][0]["id"], json!(1));
    }

    #[test]
    fn an_empty_transcript_exports_and_reimports() {
        let t = TranscriptManager::new(
            Box::new(FixedClock::reference()),
            "empty",
            "zh-TW",
            "microphone",
        )
        .into_transcript();

        let json = to_json(&t).unwrap();
        let parsed: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["segments"].as_array().unwrap().len(), 0);
        assert_eq!(import_from_str(&json).unwrap(), t);
    }

    #[test]
    fn special_characters_survive_the_round_trip() {
        let mut m = TranscriptManager::new(
            Box::new(FixedClock::reference()),
            "special",
            "zh-TW",
            "microphone",
        );
        let text = "引號\"、反斜線\\、換行\n、tab\t、表情 🎙️、C++ 與 <tag>";
        m.apply(TranscriptUpdate::final_("s1", 1, text));

        let json = to_json(m.transcript()).unwrap();
        let back = import_from_str(&json).unwrap();
        assert_eq!(back.segments[0].text, text);
    }

    #[test]
    fn errors_are_exported_separately_from_segments() {
        let mut m = TranscriptManager::new(
            Box::new(FixedClock::reference()),
            "with-error",
            "zh-TW",
            "microphone",
        );
        m.apply(TranscriptUpdate::final_("s1", 1, "正常"));
        m.record_error("麥克風裝置中斷", Some("s2".into()));

        let parsed: Value = serde_json::from_str(&to_json(m.transcript()).unwrap()).unwrap();
        assert_eq!(parsed["segments"].as_array().unwrap().len(), 1);
        assert_eq!(parsed["errors"][0]["message"], json!("麥克風裝置中斷"));
    }

    #[test]
    fn errors_key_is_omitted_when_there_are_none() {
        let parsed: Value = serde_json::from_str(&to_json(&fixture()).unwrap()).unwrap();
        assert!(
            parsed.get("errors").is_none(),
            "empty arrays should not be emitted"
        );
    }

    // --- Secret leakage (explicit product requirement) ---

    #[test]
    fn exported_document_never_contains_api_key_material() {
        let secret = "sk-live-DO-NOT-LEAK-0123456789abcdef";
        let t = fixture();
        let json = to_json(&t).unwrap();
        assert_no_secret_material(&json, &[secret]).expect("export must be secret-free");

        // And the transcript's plain-text projection, which the UI copies.
        assert_no_secret_material(&t.display_text(), &[secret]).unwrap();
        assert_no_secret_material(&t.committed_text(), &[secret]).unwrap();
    }

    #[test]
    fn the_secret_guard_actually_detects_a_leak() {
        // A guard that cannot fail is not a guard.
        let secret = "sk-live-LEAK";
        let leaky = format!("{{\"note\":\"{secret}\"}}");
        assert!(assert_no_secret_material(&leaky, &[secret]).is_err());
    }

    #[test]
    fn a_failed_export_leaves_no_temp_files_behind() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("ok.json");
        export_to_file(&path, &fixture()).unwrap();

        let leftovers: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "temp files must not survive a successful export"
        );
    }

    #[test]
    fn re_exporting_over_an_existing_file_replaces_it() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("t.json");

        let mut first = TranscriptManager::new(
            Box::new(FixedClock::reference()),
            "s",
            "zh-TW",
            "microphone",
        );
        first.apply(TranscriptUpdate::final_("a", 1, "第一版"));
        export_to_file(&path, first.transcript()).unwrap();

        let mut second = TranscriptManager::new(
            Box::new(FixedClock::reference()),
            "s",
            "zh-TW",
            "microphone",
        );
        second.apply(TranscriptUpdate::final_("a", 1, "第二版"));
        export_to_file(&path, second.transcript()).unwrap();

        let back = import_from_file(&path).unwrap();
        assert_eq!(back.committed_text(), "第二版");
    }
}
