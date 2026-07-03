//! Core data types for the Sophon downloader.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

/// Type of download operation, persisted for correct resumption dispatch.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum DownloadType {
    Fresh,
    Update,
    Preinstall,
}

/// Persisted completion summary for resume.
///
/// Newer state files store file indices (`Indices`) for compactness.
/// Older state files stored asset path strings; the deserializer emits
/// `Legacy` so the install path can migrate names to indices using the
/// freshly loaded manifest. Future saves always write `Indices`.
#[derive(Debug, Clone)]
pub enum CompletedFiles {
    Indices(Vec<usize>),
    Legacy(Vec<String>),
}

impl Default for CompletedFiles {
    fn default() -> Self {
        Self::Indices(Vec::new())
    }
}

impl Serialize for CompletedFiles {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Indices(i) => i.serialize(s),
            // Legacy names never persist; once migration runs inside
            // install(), the saver serializes Indices. Persist an empty
            // array rather than risk writing a stale legacy list.
            Self::Legacy(_) => Vec::<usize>::new().serialize(s),
        }
    }
}

impl<'de> Deserialize<'de> for CompletedFiles {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> serde::de::Visitor<'de> for V {
            type Value = CompletedFiles;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("an array of numbers (file indices) or strings (legacy paths)")
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> Result<Self::Value, A::Error> {
                let hint = seq.size_hint().unwrap_or(0);
                let mut indices: Vec<usize> = Vec::with_capacity(hint);
                let mut names: Vec<String> = Vec::with_capacity(hint);
                while let Some(v) = seq.next_element::<serde_json::Value>()? {
                    match v {
                        serde_json::Value::Number(n) => {
                            indices.push(n.as_u64().unwrap_or(0) as usize)
                        }
                        serde_json::Value::String(s) => names.push(s),
                        serde_json::Value::Null | serde_json::Value::Bool(_) => {}
                        serde_json::Value::Array(_) | serde_json::Value::Object(_) => {}
                    }
                }
                if names.is_empty() {
                    Ok(CompletedFiles::Indices(indices))
                } else {
                    // Mixed arrays are treated as legacy; numeric entries
                    // were dropped intentionally since legacy names are
                    // authoritative for migration.
                    Ok(CompletedFiles::Legacy(names))
                }
            }
        }
        d.deserialize_seq(V)
    }
}

/// Persisted state for download resumption after app restart.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadState {
    pub game_id: String,
    pub vo_lang: String,
    pub output_path: String,
    pub download_type: DownloadType,
    pub current_tag: Option<String>,
    pub manifest_hash: String,
    pub downloaded_chunks: HashMap<String, u64>,
    #[serde(default)]
    pub completed_files: CompletedFiles,
}

/// Summary of persisted download state returned to the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResumeInfo {
    pub game_id: String,
    pub download_type: DownloadType,
}

/// Save download state after every N completed chunks.
pub const CHUNK_STATE_SAVE_INTERVAL: u64 = 500;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completed_files_indices_roundtrip() {
        let v = CompletedFiles::Indices(vec![3, 7, 11, 0]);
        let json = serde_json::to_string(&v).unwrap();
        assert_eq!(json, "[3,7,11,0]");
        let back: CompletedFiles = serde_json::from_str(&json).unwrap();
        match back {
            CompletedFiles::Indices(ix) => assert_eq!(ix, vec![3, 7, 11, 0]),
            CompletedFiles::Legacy(_) => panic!("expected Indices"),
        }
    }

    #[test]
    fn completed_files_empty_serializes_to_empty_array() {
        let v = CompletedFiles::default();
        let json = serde_json::to_string(&v).unwrap();
        assert_eq!(json, "[]");
    }

    #[test]
    fn completed_files_legacy_strings_deserialize_to_legacy() {
        let json = "[\"a/b.bin\",\"c/d.dat\"]";
        let back: CompletedFiles = serde_json::from_str(json).unwrap();
        match back {
            CompletedFiles::Legacy(names) => {
                assert_eq!(names, vec!["a/b.bin".to_string(), "c/d.dat".to_string()]);
            }
            CompletedFiles::Indices(_) => panic!("expected Legacy"),
        }
    }

    #[test]
    fn completed_files_legacy_serializes_as_empty_indices() {
        let v = CompletedFiles::Legacy(vec!["stale".into()]);
        let json = serde_json::to_string(&v).unwrap();
        assert_eq!(json, "[]");
    }

    #[test]
    fn completed_files_mixed_array_keeps_strings_as_legacy() {
        let json = "[1, \"name\", 3]";
        let back: CompletedFiles = serde_json::from_str(json).unwrap();
        match back {
            // Numeric entries are dropped: when any string is present the
            // array is legacy from an older client.
            CompletedFiles::Legacy(names) => assert_eq!(names, vec!["name".to_string()]),
            CompletedFiles::Indices(_) => panic!("expected Legacy"),
        }
    }

    #[test]
    fn download_state_legacy_format_loads() {
        let json = r#"{
            "gameId":"g","voLang":"en","outputPath":"/x",
            "downloadType":"fresh","currentTag":null,"manifestHash":"h",
            "downloadedChunks":{},
            "completedFiles":["path/one","path/two"]
        }"#;
        let state: DownloadState = serde_json::from_str(json).unwrap();
        match state.completed_files {
            CompletedFiles::Legacy(names) => {
                assert_eq!(names.len(), 2);
                assert_eq!(names[0], "path/one");
            }
            CompletedFiles::Indices(_) => panic!("expected Legacy"),
        }
    }

    #[test]
    fn download_state_new_format_loads() {
        let json = r#"{
            "gameId":"g","voLang":"en","outputPath":"/x",
            "downloadType":"fresh","currentTag":null,"manifestHash":"h",
            "downloadedChunks":{},
            "completedFiles":[0,5,9]
        }"#;
        let state: DownloadState = serde_json::from_str(json).unwrap();
        match state.completed_files {
            CompletedFiles::Indices(ix) => assert_eq!(ix, vec![0, 5, 9]),
            CompletedFiles::Legacy(_) => panic!("expected Indices"),
        }
    }

    #[test]
    fn download_state_missing_completed_files_defaults_to_empty_indices() {
        let json = r#"{
            "gameId":"g","voLang":"en","outputPath":"/x",
            "downloadType":"fresh","currentTag":null,"manifestHash":"h",
            "downloadedChunks":{}
        }"#;
        let state: DownloadState = serde_json::from_str(json).unwrap();
        match state.completed_files {
            CompletedFiles::Indices(ix) => assert!(ix.is_empty()),
            CompletedFiles::Legacy(_) => panic!("expected Indices"),
        }
    }
}
