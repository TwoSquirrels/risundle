use std::path::PathBuf;

use anyhow::{Context, Result};

/// risundle の内部データを保存するローカルディレクトリ (`$LOCAL`) を表す。
///
/// `$LOCAL` は `dirs::data_local_dir()` 配下の `risundle` ディレクトリを指す。
/// ルートを差し替え可能にすることで、テストや将来の上書き設定を容易にする。
pub struct LocalStore {
    root: PathBuf,
}

impl LocalStore {
    const APP_DIR: &'static str = "risundle";
    const LIBRARIES_DIR: &'static str = "libraries";
    const TAGS_FILE: &'static str = "tags.json";
    const DUMMY_DIR: &'static str = "dummy";

    /// OS 標準のデータディレクトリから `$LOCAL` を解決する。
    pub fn discover() -> Result<Self> {
        let data_local = dirs::data_local_dir()
            .context("could not determine the OS local data directory")?;
        Ok(Self::with_root(data_local.join(Self::APP_DIR)))
    }

    /// ルートを明示して `$LOCAL` を構築する (主にテスト用)。
    pub fn with_root(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn libraries_dir(&self) -> PathBuf {
        self.root.join(Self::LIBRARIES_DIR)
    }

    pub fn library_dir(&self, id: &str) -> PathBuf {
        self.libraries_dir().join(id)
    }

    pub fn tags_json(&self, id: &str) -> PathBuf {
        self.library_dir(id).join(Self::TAGS_FILE)
    }

    pub fn dummy_dir(&self, id: &str) -> PathBuf {
        self.library_dir(id).join(Self::DUMMY_DIR)
    }

    /// `tags.json` の有無で、ライブラリが登録済みかどうかを判定する。
    pub fn is_registered(&self, id: &str) -> bool {
        self.tags_json(id).is_file()
    }

    /// `tags.json` を持つ登録済みライブラリの ID 一覧を、昇順で返す。
    ///
    /// 仕様上 `$LOCAL/libraries/*/tags.json` を列挙する箇所 (`library list`、
    /// `library update` の全件指定、バンドル時のインクルードパス収集) で用いる。
    pub fn library_ids(&self) -> Result<Vec<String>> {
        let libraries = self.libraries_dir();
        let entries = match std::fs::read_dir(&libraries) {
            Ok(entries) => entries,
            // libraries ディレクトリ自体が未作成なら、登録ゼロとして扱う。
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("failed to read {}", libraries.display()));
            }
        };

        let mut ids = Vec::new();
        for entry in entries {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            // 非 UTF-8 のディレクトリ名はライブラリ ID たり得ないためスキップ。
            let Some(id) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if self.is_registered(&id) {
                ids.push(id);
            }
        }
        ids.sort();
        Ok(ids)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::fs;
    use std::path::Path;

    use tempfile::TempDir;

    fn register(store: &LocalStore, id: &str) {
        fs::create_dir_all(store.library_dir(id)).unwrap();
        fs::write(store.tags_json(id), "{}").unwrap();
    }

    #[test]
    fn paths_are_nested_under_root() {
        let store = LocalStore::with_root("/tmp/local");
        assert_eq!(store.libraries_dir(), Path::new("/tmp/local/libraries"));
        assert_eq!(
            store.library_dir("std"),
            Path::new("/tmp/local/libraries/std")
        );
        assert_eq!(
            store.tags_json("std"),
            Path::new("/tmp/local/libraries/std/tags.json")
        );
        assert_eq!(
            store.dummy_dir("std"),
            Path::new("/tmp/local/libraries/std/dummy")
        );
    }

    #[test]
    fn library_ids_returns_empty_when_root_missing() {
        let store = LocalStore::with_root("/nonexistent/risundle/local");
        assert_eq!(store.library_ids().unwrap(), Vec::<String>::new());
    }

    #[test]
    fn library_ids_lists_only_registered_in_sorted_order() {
        let temp = TempDir::new().unwrap();
        let store = LocalStore::with_root(temp.path());

        register(&store, "std");
        register(&store, "ac-library");
        fs::create_dir_all(store.library_dir("incomplete")).unwrap();

        assert_eq!(store.library_ids().unwrap(), vec!["ac-library", "std"]);
        assert!(store.is_registered("std"));
        assert!(!store.is_registered("incomplete"));
    }
}
