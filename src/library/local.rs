use std::path::PathBuf;

use anyhow::{Context, Result, bail};

/// ライブラリ ID がパス要素として安全か検証する。
///
/// ID はそのまま `$LOCAL/libraries/<id>` のディレクトリ名になるため、空・`.`/`..`・パス区切りを
/// 含む ID を許すと意図しない場所を読み書きしてしまう。ID をパスへ変換する境界である [`LocalStore`]
/// も自ら強制するので、呼び出し元の検証忘れがストア外の読み書き (最悪 `remove_dir_all`) に繋がる
/// ことはない。受け口はフェイルファストな入力検証としてこれを直接使う。
pub fn validate_id(id: &str) -> Result<()> {
    // `:` は Windows のドライブ相対パス (`C:foo`) 対策。プレフィックス付きパスを join すると
    // ベースパスが丸ごと置き換わるため、パス区切りと同様にストアの外へ抜けられてしまう。
    if id.is_empty()
        || id == "."
        || id == ".."
        || id.contains('/')
        || id.contains('\\')
        || id.contains(':')
    {
        bail!(
            "library ID `{id}` is not allowed (empty, `.`/`..`, or IDs containing path separators or `:` are rejected)"
        );
    }
    Ok(())
}

/// risundle の内部データを保存するローカルディレクトリ (`$LOCAL`) を表す。
///
/// `$LOCAL` は `dirs::data_local_dir()` 配下の `risundle` ディレクトリを指す。
/// ルートを差し替え可能にすることで、テストや将来の上書き設定を容易にする。
#[derive(Debug)]
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
        let data_local =
            dirs::data_local_dir().context("could not determine the OS local data directory")?;
        Ok(Self::with_root(data_local.join(Self::APP_DIR)))
    }

    /// ルートを明示して `$LOCAL` を構築する (主にテスト用)。
    pub fn with_root(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn libraries_dir(&self) -> PathBuf {
        self.root.join(Self::LIBRARIES_DIR)
    }

    pub fn library_dir(&self, id: &str) -> Result<PathBuf> {
        validate_id(id)?;
        Ok(self.libraries_dir().join(id))
    }

    pub fn tags_json(&self, id: &str) -> Result<PathBuf> {
        Ok(self.library_dir(id)?.join(Self::TAGS_FILE))
    }

    pub fn dummy_dir(&self, id: &str) -> Result<PathBuf> {
        Ok(self.library_dir(id)?.join(Self::DUMMY_DIR))
    }

    /// `tags.json` の有無で、ライブラリが登録済みかどうかを判定する。不正な ID は登録され得ない
    /// ので false を返す。
    pub fn is_registered(&self, id: &str) -> bool {
        self.tags_json(id).is_ok_and(|path| path.is_file())
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
        fs::create_dir_all(store.library_dir(id).unwrap()).unwrap();
        fs::write(store.tags_json(id).unwrap(), "{}").unwrap();
    }

    #[test]
    fn paths_are_nested_under_root() {
        let store = LocalStore::with_root("/tmp/local");
        assert_eq!(store.libraries_dir(), Path::new("/tmp/local/libraries"));
        assert_eq!(
            store.library_dir("std").unwrap(),
            Path::new("/tmp/local/libraries/std")
        );
        assert_eq!(
            store.tags_json("std").unwrap(),
            Path::new("/tmp/local/libraries/std/tags.json")
        );
        assert_eq!(
            store.dummy_dir("std").unwrap(),
            Path::new("/tmp/local/libraries/std/dummy")
        );
    }

    #[test]
    fn rejects_ids_that_escape_the_store() {
        let store = LocalStore::with_root("/tmp/local");
        for bad in ["", ".", "..", "../evil", "a/b", "a\\b", "C:foo"] {
            assert!(store.library_dir(bad).is_err(), "{bad} を弾くべき");
            assert!(!store.is_registered(bad));
        }
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
        fs::create_dir_all(store.library_dir("incomplete").unwrap()).unwrap();

        assert_eq!(store.library_ids().unwrap(), vec!["ac-library", "std"]);
        assert!(store.is_registered("std"));
        assert!(!store.is_registered("incomplete"));
    }

    #[test]
    fn library_ids_reports_an_unreadable_libraries_dir() {
        // NotFound (未作成 = 登録ゼロ) 以外の読み取りエラーは握り潰さず文脈付きで返す。
        let temp = TempDir::new().unwrap();
        let store = LocalStore::with_root(temp.path());
        fs::write(store.libraries_dir(), "").unwrap(); // ディレクトリの位置にファイル

        assert!(store.library_ids().is_err());
    }

    #[test]
    fn library_ids_skips_stray_files() {
        let temp = TempDir::new().unwrap();
        let store = LocalStore::with_root(temp.path());
        register(&store, "lib");
        fs::write(store.libraries_dir().join("README.txt"), "").unwrap();

        assert_eq!(store.library_ids().unwrap(), vec!["lib"]);
    }

    #[cfg(unix)]
    #[test]
    fn library_ids_skips_non_utf8_names() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let temp = TempDir::new().unwrap();
        let store = LocalStore::with_root(temp.path());
        register(&store, "lib");
        fs::create_dir_all(store.libraries_dir().join(OsStr::from_bytes(b"\xff"))).unwrap();

        assert_eq!(store.library_ids().unwrap(), vec!["lib"]);
    }
}
