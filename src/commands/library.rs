//! `library` サブコマンドの受け口。入力の検証・表示の整形・成功メッセージ (標準出力) を担い、
//! 登録の実処理は [`crate::library::registry`] に任せる。

use std::path::Path;

use anyhow::{Result, bail};

use crate::cli::LibraryCommand;
use crate::config::Config;
use crate::library::local::{LocalStore, validate_id};
use crate::library::registry::{self, STD_ID};
use crate::library::tags::{Registration, SchemaMismatch, Tags, TagsKind};

pub fn run(command: LibraryCommand) -> Result<()> {
    let store = LocalStore::discover()?;
    match command {
        LibraryCommand::Add { id, path } => add(&store, &id, &path),
        LibraryCommand::AddStd { compiler } => add_std(&store, compiler.as_deref()),
        LibraryCommand::Delete { id } => delete(&store, &id),
        LibraryCommand::Update { id, path } => update(&store, id.as_deref(), path.as_deref()),
        LibraryCommand::List => list(&store),
        LibraryCommand::Show { id, verbose } => show(&store, &id, verbose),
    }
}

fn add(store: &LocalStore, id: &str, path: &Path) -> Result<()> {
    validate_id(id)?;
    if id == STD_ID {
        bail!("register the standard library with `risundle library add-std`");
    }
    if store.is_registered(id) {
        bail!(
            "library `{id}` is already registered; use `risundle library update {id}` to update it"
        );
    }
    let source_root = registry::resolve_source_root(path)?;

    eprintln!("registering library `{id}`...");
    registry::register(store, id, &source_root)?;

    println!("registered library `{id}`");
    Ok(())
}

/// コンパイラ省略時は組み込みデフォルトを要求として渡す。デフォルトの決定は設定 (環境側の関心事)
/// なので受け口が担い、認識集合の育て方は registry に任せる。
fn add_std(store: &LocalStore, compiler: Option<&Path>) -> Result<()> {
    let requested = compiler.map_or_else(|| Config::default().compiler, Path::to_path_buf);
    let count = registry::add_std(store, &requested)?;

    println!("registered the standard library (`{STD_ID}`) for {count} compiler(s)");
    Ok(())
}

fn ensure_registered(store: &LocalStore, id: &str) -> Result<()> {
    if !store.is_registered(id) {
        bail!("library `{id}` is not registered");
    }
    Ok(())
}

fn delete(store: &LocalStore, id: &str) -> Result<()> {
    validate_id(id)?;
    ensure_registered(store, id)?;
    registry::remove(store, id)?;

    println!("removed registration of library `{id}`");
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
                println!("no libraries to update");
                return Ok(());
            }
            for id in ids {
                update_one(store, &id, None)?;
            }
            Ok(())
        }
    }
}

/// 通常ライブラリは `path` 省略時に保存済みパスを再利用し、`std` は保存済みコンパイラからシステム
/// include パスを再検出する。
fn update_one(store: &LocalStore, id: &str, path: Option<&Path>) -> Result<()> {
    validate_id(id)?;
    ensure_registered(store, id)?;
    eprintln!("updating library `{id}`...");
    registry::reregister(store, id, path)?;
    println!("updated library `{id}`");
    Ok(())
}

fn list(store: &LocalStore) -> Result<()> {
    let ids = store.library_ids()?;
    if ids.is_empty() {
        println!("no libraries are registered");
        return Ok(());
    }
    // ID・種別・パスしか出さず、いずれも全スキーマバージョンに共通のため、スキーマ検証をしない
    // Registration で読む。一覧はアップグレード直後でもエラーにせず動くべき (バンドル時に自動移行)。
    // 種別を足しつつタブ区切りを保ち、grep/awk などでのパイプ処理を妨げない。
    for id in ids {
        let reg = Registration::load(&store.tags_json(&id)?)?;
        println!("{id}\t{}\t{}", reg.kind_label(), reg.path.display());
    }
    Ok(())
}

/// `show` の 1 項目を、ラベル幅を揃えて出力する。最長ラベル `Compilers` に合わせる。
fn show_field(label: &str, value: &str) {
    println!("{label:<9} {value}");
}

fn show(store: &LocalStore, id: &str, verbose: bool) -> Result<()> {
    validate_id(id)?;
    ensure_registered(store, id)?;
    match Tags::load(&store.tags_json(id)?) {
        Ok(tags) => show_tags(id, &tags, verbose),
        // 詳細 (定義識別子・ハッシュ) は現行スキーマでないと読めない。読み取りコマンドが状態を
        // 書き換えるのは避けたいので、自動移行はせず、読める基本情報だけ出して update を案内する。
        Err(err) => match err.downcast_ref::<SchemaMismatch>() {
            Some(mismatch) => show_outdated(store, id, &mismatch.to_string()),
            None => Err(err),
        },
    }
}

/// スキーマが古く詳細を読めない登録について、`Registration` から読める基本情報だけ表示する。
fn show_outdated(store: &LocalStore, id: &str, reason: &str) -> Result<()> {
    let reg = Registration::load(&store.tags_json(id)?)?;
    show_field("ID", id);
    show_field("Path", &reg.path.display().to_string());
    show_field("Kind", reg.kind_label());
    show_field("Details", &format!("unavailable ({reason})"));
    Ok(())
}

fn show_tags(id: &str, tags: &Tags, verbose: bool) -> Result<()> {
    show_field("ID", id);
    show_field("Path", &tags.path.display().to_string());
    match &tags.kind {
        TagsKind::Std { compilers } => {
            show_field("Kind", "std (no identifier info or update detection)");
            show_field("Compilers", &compilers.len().to_string());
            for compiler in compilers {
                println!("  {}", compiler.display());
            }
        }
        TagsKind::Library {
            hash,
            files,
            implements,
        } => {
            show_field("Kind", "library");
            show_field(
                "Files",
                &format!("{} with defined identifiers", files.len()),
            );
            if verbose {
                show_field("Hash", hash);
                println!("Definitions:");
                for (file, names) in files {
                    println!("  {file}: {}", names.join(", "));
                }
                if !implements.is_empty() {
                    println!("Implements:");
                    for (file, names) in implements {
                        println!("  {file}: {}", names.join(", "));
                    }
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

    use crate::library::testutil::{downgrade_schema, source_with, store_in};

    #[test]
    fn add_rejects_std_id() {
        // std は専用の add-std で登録する。汎用 add は弾く。
        let local = TempDir::new().unwrap();
        let store = store_in(&local);
        let source = source_with(&[("vector", "// std header")]);

        assert!(add(&store, "std", source.path()).is_err());
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
        assert!(!store.library_dir("lib").unwrap().exists());
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

        let tags = Tags::load(&store.tags_json("lib").unwrap()).unwrap();
        match tags.kind {
            TagsKind::Library { files, .. } => {
                assert!(files.contains_key("atcoder/fenwick.hpp"));
            }
            TagsKind::Std { .. } => panic!("非 std ライブラリは Library を持つべき"),
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

        let tags = Tags::load(&store.tags_json("lib").unwrap()).unwrap();
        assert_eq!(tags.path, moved.path().canonicalize().unwrap());
        assert!(store.dummy_dir("lib").unwrap().join("b.hpp").is_file());
        assert!(!store.dummy_dir("lib").unwrap().join("a.hpp").is_file());
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
    fn list_and_show_tolerate_outdated_schema() {
        let local = TempDir::new().unwrap();
        let store = store_in(&local);
        let source = source_with(&[("vec.hpp", "struct Vec {};")]);
        add(&store, "mylib", source.path()).unwrap();
        downgrade_schema(&store, "mylib");

        // 読み取りコマンドはスキーマ不一致でもエラーにせず、移行もしない (状態は古いまま)。
        list(&store).unwrap();
        show(&store, "mylib", true).unwrap();
        assert!(
            Tags::load(&store.tags_json("mylib").unwrap()).is_err(),
            "読み取りコマンドは登録を書き換えない"
        );
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
