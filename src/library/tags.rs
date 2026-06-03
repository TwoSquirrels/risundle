//! `tags.json` の読み書きを担うデータ層。ライブラリ管理ロジックとは独立し、
//! データ構造の表現とシリアライズ/デシリアライズのみを責務とする。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

const CURRENT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tags {
    pub path: PathBuf,
    pub kind: TagsKind,
}

/// ライブラリ種別ごとに保持する情報。
///
/// `std` は識別子情報を持たず更新検知の対象外のため `hash`・`files` を持たない。代わりに、システム
/// include パスの自動検出に用いた `compilers` (正規化済み絶対パスの集合) を保持する。複数コンパイラの
/// std を 1 つのダミーツリーへ統合するため集合で持ち、`update` 時の再検出とバンドル時の整合警告に使う。
/// この直和により「`std`」と「識別子 0 件のライブラリ」を型レベルで区別する。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagsKind {
    Std {
        compilers: Vec<PathBuf>,
    },
    Library {
        hash: String,
        // 出力順を安定させるため BTreeMap を使う (HashMap だと tags.json の diff が毎回ぶれる)。
        files: BTreeMap<String, Vec<String>>,
    },
}

impl Tags {
    pub fn from_json(json: &str) -> Result<Self> {
        let raw: RawTags = serde_json::from_str(json)?;
        raw.try_into()
    }

    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string_pretty(&RawTags::from(self))
            .context("tags.json のシリアライズに失敗しました")
    }

    pub fn load(path: &Path) -> Result<Self> {
        let json = std::fs::read_to_string(path)
            .with_context(|| format!("{} の読み込みに失敗しました", path.display()))?;
        Self::from_json(&json).with_context(|| format!("{} の解釈に失敗しました", path.display()))
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let json = self.to_json()?;
        std::fs::write(path, json)
            .with_context(|| format!("{} の書き込みに失敗しました", path.display()))
    }
}

/// `tags.json` の生のシリアライズ表現。`std` は `compiler` のみ、通常ライブラリは `hash`・`files` を
/// 持つ。種別ごとに排他なので、どちらの組が揃っているかで [`TagsKind`] を復元する。
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTags {
    schema_version: u32,
    path: PathBuf,
    #[serde(skip_serializing_if = "Option::is_none")]
    compilers: Option<Vec<PathBuf>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    files: Option<BTreeMap<String, Vec<String>>>,
}

impl TryFrom<RawTags> for Tags {
    type Error = anyhow::Error;

    fn try_from(raw: RawTags) -> Result<Self> {
        if raw.schema_version != CURRENT_SCHEMA_VERSION {
            bail!(
                "tags.json の schema_version {} は非対応です (対応: {})。`risundle library update` で再生成してください",
                raw.schema_version,
                CURRENT_SCHEMA_VERSION
            );
        }
        let kind = match (raw.compilers, raw.hash, raw.files) {
            (Some(compilers), None, None) => TagsKind::Std { compilers },
            (None, Some(hash), Some(files)) => TagsKind::Library { hash, files },
            _ => {
                bail!(
                    "tags.json の種別が不正です (std は compilers のみ、通常ライブラリは hash と files の両方が必要です)"
                );
            }
        };
        Ok(Self {
            path: raw.path,
            kind,
        })
    }
}

impl From<&Tags> for RawTags {
    fn from(tags: &Tags) -> Self {
        let (compilers, hash, files) = match &tags.kind {
            TagsKind::Std { compilers } => (Some(compilers.clone()), None, None),
            TagsKind::Library { hash, files } => (None, Some(hash.clone()), Some(files.clone())),
        };
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            path: tags.path.clone(),
            compilers,
            hash,
            files,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use tempfile::TempDir;

    fn library_tags() -> Tags {
        Tags {
            path: PathBuf::from("/usr/local/include"),
            kind: TagsKind::Library {
                hash: "sha256:abc".to_owned(),
                files: BTreeMap::from([
                    ("atcoder/modint.hpp".to_owned(), vec!["modint".to_owned()]),
                    ("atcoder/segtree.hpp".to_owned(), vec!["segtree".to_owned()]),
                ]),
            },
        }
    }

    fn std_tags() -> Tags {
        Tags {
            path: PathBuf::from("/usr/include/c++/12"),
            kind: TagsKind::Std {
                compilers: vec![
                    PathBuf::from("/usr/bin/g++"),
                    PathBuf::from("/usr/bin/clang++"),
                ],
            },
        }
    }

    #[test]
    fn std_round_trips_through_json() {
        let tags = std_tags();
        assert_eq!(Tags::from_json(&tags.to_json().unwrap()).unwrap(), tags);
    }

    #[test]
    fn library_round_trips_through_json() {
        let tags = library_tags();
        assert_eq!(Tags::from_json(&tags.to_json().unwrap()).unwrap(), tags);
    }

    #[test]
    fn std_omits_hash_and_files_in_output() {
        let json = std_tags().to_json().unwrap();
        assert!(!json.contains("hash"));
        assert!(!json.contains("files"));
        assert!(json.contains("compilers"));
    }

    #[test]
    fn library_with_no_identifiers_keeps_empty_files_object() {
        let tags = Tags {
            path: PathBuf::from("/usr/local/include"),
            kind: TagsKind::Library {
                hash: "sha256:abc".to_owned(),
                files: BTreeMap::new(),
            },
        };
        let json = tags.to_json().unwrap();
        assert!(json.contains("\"files\": {}"));
        assert_eq!(Tags::from_json(&json).unwrap(), tags);
    }

    #[test]
    fn parses_spec_std_example() {
        let json = r#"{ "schema_version": 1, "path": "/usr/include/c++/12", "compilers": ["/usr/bin/g++"] }"#;
        let tags = Tags::from_json(json).unwrap();
        assert_eq!(
            tags.kind,
            TagsKind::Std {
                compilers: vec![PathBuf::from("/usr/bin/g++")]
            }
        );
    }

    #[test]
    fn unsupported_schema_version_is_rejected() {
        let json = r#"{ "schema_version": 2, "path": "/usr/include/c++/12", "compilers": ["/usr/bin/g++"] }"#;
        let error = Tags::from_json(json).unwrap_err();
        assert!(error.to_string().contains("schema_version"));
    }

    #[test]
    fn std_without_compilers_is_rejected() {
        // compilers も hash/files も無い旧 std 形式は不正 (add-std での再登録が必要)。
        let json = r#"{ "schema_version": 1, "path": "/usr/include/c++/12" }"#;
        assert!(Tags::from_json(json).is_err());
    }

    #[test]
    fn hash_without_files_is_rejected() {
        let json = r#"{ "schema_version": 1, "path": "/p", "hash": "sha256:abc" }"#;
        assert!(Tags::from_json(json).is_err());
    }

    #[test]
    fn files_without_hash_is_rejected() {
        let json = r#"{ "schema_version": 1, "path": "/p", "files": {} }"#;
        assert!(Tags::from_json(json).is_err());
    }

    #[test]
    fn std_with_hash_is_rejected() {
        // std (compilers) と通常ライブラリ (hash/files) の情報が混在する形式は不正。
        let json = r#"{ "schema_version": 1, "path": "/p", "compilers": ["/usr/bin/g++"], "hash": "sha256:abc" }"#;
        assert!(Tags::from_json(json).is_err());
    }

    #[test]
    fn unknown_field_is_rejected() {
        let json = r#"{ "schema_version": 1, "path": "/p", "unknown": true }"#;
        assert!(Tags::from_json(json).is_err());
    }

    #[test]
    fn save_then_load_preserves_tags() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("tags.json");
        let tags = library_tags();

        tags.save(&path).unwrap();
        assert_eq!(Tags::load(&path).unwrap(), tags);
    }
}
