use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::cli::LibraryCommand;
use crate::local::LocalStore;
use crate::tags::{Tags, TagsKind};
use crate::{dummy, hash, identifiers};

/// `std` として扱うライブラリ ID。識別子情報を持たず、更新検知の対象外とする。
const STD_ID: &str = "std";

pub fn run(command: LibraryCommand) -> Result<()> {
    let store = LocalStore::discover()?;
    match command {
        LibraryCommand::Add { id, path } => add(&store, &id, &path),
        LibraryCommand::Delete { id } => delete(&store, &id),
        LibraryCommand::Update { id, path } => update(&store, id.as_deref(), path.as_deref()),
        LibraryCommand::List => list(&store),
        LibraryCommand::Show { id } => show(&store, &id),
    }
}

fn add(store: &LocalStore, id: &str, path: &Path) -> Result<()> {
    validate_id(id)?;
    if store.is_registered(id) {
        bail!(
            "ライブラリ `{id}` は既に登録されています。更新するには `risundle library update {id}` を使ってください"
        );
    }

    // 絶対パス化を兼ねて存在を確認する (canonicalize は存在しないパスでエラーになる)。
    // tags.json には絶対パスを保存する必要があるため、ここで解決したものを一貫して使う。
    let source_root = path
        .canonicalize()
        .with_context(|| format!("インクルードパス {} を解決できませんでした", path.display()))?;

    let library_dir = store.library_dir(id);
    if library_dir.exists() {
        // 前回の登録失敗などで残った不完全なディレクトリを作り直す。
        std::fs::remove_dir_all(&library_dir)
            .with_context(|| format!("{} の削除に失敗しました", library_dir.display()))?;
    }
    std::fs::create_dir_all(&library_dir)
        .with_context(|| format!("{} の作成に失敗しました", library_dir.display()))?;

    eprintln!("ライブラリ `{id}` を登録しています...");
    dummy::generate(&source_root, &store.dummy_dir(id))?;

    // std は識別子情報を持たず更新検知の対象外。それ以外は files と hash の両方を必ず持つ。
    let kind = if id == STD_ID {
        TagsKind::Std
    } else {
        // 識別子抽出はファイル数に比例して時間がかかるため、処理中のファイル名を逐次表示する。
        let files = identifiers::enumerate(&source_root, |relative| eprintln!("  {relative}"))?;
        let hash = hash::aggregate(&source_root)?;
        TagsKind::Library { hash, files }
    };
    Tags {
        path: source_root,
        kind,
    }
    .save(&store.tags_json(id))?;

    println!("ライブラリ `{id}` を登録しました");
    Ok(())
}

/// ライブラリ ID がパス要素として安全か検証する。
///
/// ID はそのまま `$LOCAL/libraries/<id>` のディレクトリ名になるため、空・`.`/`..`・パス区切りを
/// 含む ID を許すと意図しない場所を読み書きしてしまう。フェイルファストで早期に弾く。
fn validate_id(id: &str) -> Result<()> {
    if id.is_empty() || id == "." || id == ".." || id.contains('/') || id.contains('\\') {
        bail!("ライブラリ ID `{id}` は使えません (空、`.`/`..`、パス区切りを含む ID は不可です)");
    }
    Ok(())
}

fn delete(_store: &LocalStore, _id: &str) -> Result<()> {
    bail!("`library delete` は未実装です");
}

fn update(_store: &LocalStore, _id: Option<&str>, _path: Option<&Path>) -> Result<()> {
    bail!("`library update` は未実装です");
}

fn list(_store: &LocalStore) -> Result<()> {
    bail!("`library list` は未実装です");
}

fn show(_store: &LocalStore, _id: &str) -> Result<()> {
    bail!("`library show` は未実装です");
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::fs;

    use tempfile::TempDir;

    fn source_with(files: &[(&str, &str)]) -> TempDir {
        let temp = TempDir::new().unwrap();
        for (relative, content) in files {
            let path = temp.path().join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, content).unwrap();
        }
        temp
    }

    fn store_in(local: &TempDir) -> LocalStore {
        LocalStore::with_root(local.path())
    }

    #[test]
    fn registers_non_std_library_with_files_dummy_and_hash() {
        let local = TempDir::new().unwrap();
        let store = store_in(&local);
        let source = source_with(&[("atcoder/modint.hpp", "struct modint {};")]);

        add(&store, "ac-library", source.path()).unwrap();

        assert!(store.is_registered("ac-library"));
        let tags = Tags::load(&store.tags_json("ac-library")).unwrap();
        match tags.kind {
            TagsKind::Library { hash, files } => {
                assert!(hash.starts_with("sha256:"));
                assert!(files["atcoder/modint.hpp"].contains(&"modint".to_owned()));
            }
            TagsKind::Std => panic!("非 std ライブラリは Library を持つべき"),
        }
        assert!(
            store
                .dummy_dir("ac-library")
                .join("atcoder/modint.hpp")
                .is_file()
        );
    }

    #[test]
    fn registers_std_without_files_or_hash() {
        let local = TempDir::new().unwrap();
        let store = store_in(&local);
        let source = source_with(&[("vector", "// std header")]);

        add(&store, "std", source.path()).unwrap();

        let tags = Tags::load(&store.tags_json("std")).unwrap();
        assert_eq!(tags.kind, TagsKind::Std);
    }

    #[test]
    fn rejects_already_registered_id() {
        let local = TempDir::new().unwrap();
        let store = store_in(&local);
        let source = source_with(&[("a.hpp", "int a;")]);

        add(&store, "lib", source.path()).unwrap();
        assert!(add(&store, "lib", source.path()).is_err());
    }

    #[test]
    fn rejects_invalid_id() {
        let local = TempDir::new().unwrap();
        let store = store_in(&local);
        let source = source_with(&[("a.hpp", "int a;")]);

        for bad in ["", ".", "..", "../evil", "a/b"] {
            assert!(add(&store, bad, source.path()).is_err(), "{bad} を弾くべき");
        }
    }

    #[test]
    fn rejects_missing_path() {
        let local = TempDir::new().unwrap();
        let store = store_in(&local);
        assert!(add(&store, "lib", &local.path().join("nonexistent")).is_err());
    }
}
