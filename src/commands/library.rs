use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::cli::LibraryCommand;
use crate::library::local::LocalStore;
use crate::library::tags::{Tags, TagsKind};
use crate::library::{dummy, hash, identifiers};

/// `std` として扱うライブラリ ID。識別子情報を持たず、更新検知の対象外とする。
const STD_ID: &str = "std";

pub fn run(command: LibraryCommand) -> Result<()> {
    let store = LocalStore::discover()?;
    match command {
        LibraryCommand::Add { id, path } => add(&store, &id, &path),
        LibraryCommand::Delete { id } => delete(&store, &id),
        LibraryCommand::Update { id, path } => update(&store, id.as_deref(), path.as_deref()),
        LibraryCommand::List => list(&store),
        LibraryCommand::Show { id, verbose } => show(&store, &id, verbose),
    }
}

fn add(store: &LocalStore, id: &str, path: &Path) -> Result<()> {
    validate_id(id)?;
    if store.is_registered(id) {
        bail!(
            "ライブラリ `{id}` は既に登録されています。更新するには `risundle library update {id}` を使ってください"
        );
    }
    let source_root = resolve_source_root(path)?;

    eprintln!("ライブラリ `{id}` を登録しています...");
    register(store, id, &source_root)?;

    println!("ライブラリ `{id}` を登録しました");
    Ok(())
}

/// `id` のライブラリディレクトリを作り直し、ダミー・`tags.json` を生成する。`add` と `update` の中核。
///
/// `source_root` は解決済みの絶対パスを前提とする (`tags.json` にそのまま保存するため)。既存の
/// ディレクトリは丸ごと作り直すので、登録失敗で残った不完全な状態や、更新前の古い内容を引きずらない。
fn register(store: &LocalStore, id: &str, source_root: &Path) -> Result<()> {
    let library_dir = store.library_dir(id);
    if library_dir.exists() {
        std::fs::remove_dir_all(&library_dir)
            .with_context(|| format!("{} の削除に失敗しました", library_dir.display()))?;
    }
    std::fs::create_dir_all(&library_dir)
        .with_context(|| format!("{} の作成に失敗しました", library_dir.display()))?;

    dummy::generate(source_root, &store.dummy_dir(id))?;

    // std は識別子情報を持たず更新検知の対象外。それ以外は files と hash の両方を必ず持つ。
    let kind = if id == STD_ID {
        TagsKind::Std
    } else {
        // 識別子抽出はファイル数に比例して時間がかかるため、処理中のファイル名を逐次表示する。
        let files = identifiers::enumerate(source_root, |relative| eprintln!("  {relative}"))?;
        let hash = hash::aggregate(source_root)?;
        TagsKind::Library { hash, files }
    };
    Tags {
        path: source_root.to_path_buf(),
        kind,
    }
    .save(&store.tags_json(id))
}

/// インクルードパスを絶対パスへ解決する。`canonicalize` は存在しないパスでエラーになるため、
/// 絶対パス化と存在確認を兼ねる。
fn resolve_source_root(path: &Path) -> Result<PathBuf> {
    path.canonicalize()
        .with_context(|| format!("インクルードパス {} を解決できませんでした", path.display()))
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

/// ライブラリが登録済みであることを確認する。delete / update / show が処理前に呼ぶ。
fn ensure_registered(store: &LocalStore, id: &str) -> Result<()> {
    if !store.is_registered(id) {
        bail!("ライブラリ `{id}` は登録されていません");
    }
    Ok(())
}

fn delete(store: &LocalStore, id: &str) -> Result<()> {
    validate_id(id)?;
    ensure_registered(store, id)?;
    let library_dir = store.library_dir(id);
    std::fs::remove_dir_all(&library_dir)
        .with_context(|| format!("{} の削除に失敗しました", library_dir.display()))?;

    println!("ライブラリ `{id}` の登録を削除しました");
    Ok(())
}

/// `id` 省略時は登録済みの全ライブラリを更新する。`path` を伴う省略は clap の positional 順序上
/// 起こり得ない (`path` は `id` の後ろにしか来ない)。
fn update(store: &LocalStore, id: Option<&str>, path: Option<&Path>) -> Result<()> {
    match id {
        Some(id) => update_one(store, id, path),
        None => {
            let ids = store.library_ids()?;
            if ids.is_empty() {
                println!("更新するライブラリがありません");
                return Ok(());
            }
            for id in ids {
                update_one(store, &id, None)?;
            }
            Ok(())
        }
    }
}

/// 1 つのライブラリを再生成する。`path` 省略時は `tags.json` に保存済みのパスを再利用する。
fn update_one(store: &LocalStore, id: &str, path: Option<&Path>) -> Result<()> {
    validate_id(id)?;
    ensure_registered(store, id)?;
    let source_root = match path {
        Some(path) => resolve_source_root(path)?,
        None => Tags::load(&store.tags_json(id))?.path,
    };

    eprintln!("ライブラリ `{id}` を更新しています...");
    register(store, id, &source_root)?;

    println!("ライブラリ `{id}` を更新しました");
    Ok(())
}

fn list(store: &LocalStore) -> Result<()> {
    let ids = store.library_ids()?;
    if ids.is_empty() {
        println!("登録済みのライブラリはありません");
        return Ok(());
    }
    for id in ids {
        let tags = Tags::load(&store.tags_json(&id))?;
        println!("{id}\t{}", tags.path.display());
    }
    Ok(())
}

fn show(store: &LocalStore, id: &str, verbose: bool) -> Result<()> {
    validate_id(id)?;
    ensure_registered(store, id)?;
    let tags = Tags::load(&store.tags_json(id))?;
    println!("ID:   {id}");
    println!("パス: {}", tags.path.display());
    match &tags.kind {
        TagsKind::Std => println!("種別: 標準ライブラリ (識別子情報・更新検知なし)"),
        TagsKind::Library { hash, files } => {
            println!("定義識別子を持つファイル: {} 件", files.len());
            if verbose {
                println!("ハッシュ: {hash}");
                for (file, names) in files {
                    println!("  {file}: {}", names.join(", "));
                }
            }
        }
    }
    Ok(())
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

    #[test]
    fn delete_removes_registered_library() {
        let local = TempDir::new().unwrap();
        let store = store_in(&local);
        let source = source_with(&[("a.hpp", "int a;")]);

        add(&store, "lib", source.path()).unwrap();
        delete(&store, "lib").unwrap();

        assert!(!store.is_registered("lib"));
        assert!(!store.library_dir("lib").exists());
    }

    #[test]
    fn delete_errors_when_not_registered() {
        let local = TempDir::new().unwrap();
        let store = store_in(&local);
        assert!(delete(&store, "lib").is_err());
    }

    #[test]
    fn update_reuses_stored_path_and_picks_up_changes() {
        let local = TempDir::new().unwrap();
        let store = store_in(&local);
        let source = source_with(&[("atcoder/dsu.hpp", "struct dsu {};")]);

        add(&store, "lib", source.path()).unwrap();

        // 元のパスにファイルを追加し、path 省略の update が再走査することを確かめる。
        fs::write(
            source.path().join("atcoder/fenwick.hpp"),
            "struct fenwick {};",
        )
        .unwrap();
        update(&store, Some("lib"), None).unwrap();

        let tags = Tags::load(&store.tags_json("lib")).unwrap();
        match tags.kind {
            TagsKind::Library { files, .. } => {
                assert!(files.contains_key("atcoder/fenwick.hpp"));
            }
            TagsKind::Std => panic!("非 std ライブラリは Library を持つべき"),
        }
    }

    #[test]
    fn update_with_new_path_reregisters_from_it() {
        let local = TempDir::new().unwrap();
        let store = store_in(&local);
        let original = source_with(&[("a.hpp", "struct a {};")]);
        let moved = source_with(&[("b.hpp", "struct b {};")]);

        add(&store, "lib", original.path()).unwrap();
        update(&store, Some("lib"), Some(moved.path())).unwrap();

        let tags = Tags::load(&store.tags_json("lib")).unwrap();
        assert_eq!(tags.path, moved.path().canonicalize().unwrap());
        assert!(store.dummy_dir("lib").join("b.hpp").is_file());
        assert!(!store.dummy_dir("lib").join("a.hpp").is_file());
    }

    #[test]
    fn update_all_refreshes_every_library() {
        let local = TempDir::new().unwrap();
        let store = store_in(&local);
        let first = source_with(&[("a.hpp", "struct a {};")]);
        let second = source_with(&[("b.hpp", "struct b {};")]);

        add(&store, "first", first.path()).unwrap();
        add(&store, "second", second.path()).unwrap();

        update(&store, None, None).unwrap();

        assert!(store.is_registered("first"));
        assert!(store.is_registered("second"));
    }

    #[test]
    fn update_errors_when_not_registered() {
        let local = TempDir::new().unwrap();
        let store = store_in(&local);
        assert!(update(&store, Some("lib"), None).is_err());
    }

    #[test]
    fn list_and_show_succeed_for_registered_libraries() {
        let local = TempDir::new().unwrap();
        let store = store_in(&local);
        let source = source_with(&[("atcoder/modint.hpp", "struct modint {};")]);

        add(&store, "ac-library", source.path()).unwrap();

        list(&store).unwrap();
        show(&store, "ac-library", false).unwrap();
        show(&store, "ac-library", true).unwrap();
    }

    #[test]
    fn show_errors_when_not_registered() {
        let local = TempDir::new().unwrap();
        let store = store_in(&local);
        assert!(show(&store, "lib", false).is_err());
    }
}
