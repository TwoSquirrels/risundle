//! ライブラリ登録の処理。ダミー生成と `tags.json` の書き込みという副作用そのものが目的の処理
//! なので、純粋な計算を核に持つ `bundle` と違い、コンパイラ起動やファイル書き込みの IO ごと
//! ここが抱える。時間のかかる処理の途中経過は標準エラーへ逐次出し、ユーザーへの最終結果
//! (標準出力) は受け口 (`commands/library.rs`) が出す。

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::compiler::{self, system_includes};
use crate::library::local::LocalStore;
use crate::library::tags::{Registration, SchemaMismatch, Tags, TagsKind};
use crate::library::{dummy, hash, identifiers};

/// `std` として扱うライブラリ ID。識別子情報を持たず、更新検知の対象外とする。
pub const STD_ID: &str = "std";

/// 通常ライブラリのディレクトリを作り直し、ダミー・`tags.json` (hash + files) を生成する。
///
/// `source_root` は [`resolve_source_root`] で解決済みの絶対パスを前提とする (`tags.json` にそのまま
/// 保存するため)。既存のディレクトリは丸ごと作り直すので、登録失敗で残った不完全な状態や、更新前の
/// 古い内容を引きずらない。
pub fn register(store: &LocalStore, id: &str, source_root: &Path) -> Result<()> {
    // 受け口の同名チェックは案内のための早期検証で、こちらは公開 API としての不変条件。std の
    // 登録を Library 種別で上書きされると、認識コンパイラ集合の喪失など静かな壊れ方をする。
    if id == STD_ID {
        bail!("register the standard library with `risundle library add-std`");
    }
    recreate_library_dir(store, id)?;
    dummy::generate(source_root, &store.dummy_dir(id)?)?;

    // 識別子抽出はファイル数に比例して時間がかかるため、処理中のファイル名を逐次表示する。
    let names = identifiers::enumerate(source_root, |relative| eprintln!("  {relative}"))?;
    let hash = hash::aggregate(source_root)?;
    Tags {
        path: source_root.to_path_buf(),
        kind: TagsKind::Library {
            hash,
            files: names.definitions,
            implements: names.implements,
        },
    }
    .save(&store.tags_json(id)?)
}

/// `std` に 1 つのコンパイラを加えて登録を作り直し、認識集合のコンパイラ数を返す。
///
/// 単一のグローバルコンパイラを握るのではなく「認識しているコンパイラの集合」を育てる方針。集合の全
/// コンパイラのシステム include パスを 1 つのダミーツリーへ統合するため、どのコンパイラでバンドルしても
/// 解決でき、背反が起きない。コンパイラは絶対パスへ正規化して表記揺れ (`g++` と `/usr/bin/g++`) を防ぐ。
pub fn add_std(store: &LocalStore, requested: &Path) -> Result<usize> {
    let resolved = compiler::resolve(requested)?;

    let mut compilers = existing_std_compilers(store)?;
    if !compilers.contains(&resolved) {
        compilers.push(resolved);
    }

    // 進捗表示は検証 (コンパイラ解決・既存登録の読み込み) を抜けてから。失敗する呼び出しに
    // 「registering...」を見せない。
    eprintln!("registering the standard library...");
    let discovered = discover_all(&compilers)?;
    register_std(store, &discovered)?;
    Ok(compilers.len())
}

const AUTO_DETECT_CANDIDATES: &[&str] = &["g++", "clang++"];

/// std が未登録なら、PATH 内の候補コンパイラ (g++/clang++) を自動検出して登録する。
///
/// バンドル実行の初回セットアップ用。コンパイラが 1 つも見つからなければ何もしない
/// (バンドル側の std 警告が案内する)。既に登録済みならスキップする。
pub fn auto_setup_std(store: &LocalStore) -> Result<()> {
    if store.is_registered(STD_ID) {
        return Ok(());
    }
    let compilers: Vec<PathBuf> = AUTO_DETECT_CANDIDATES
        .iter()
        .filter_map(|name| compiler::resolve(Path::new(name)).ok())
        .collect();
    if compilers.is_empty() {
        return Ok(());
    }
    eprintln!(
        "initial setup: registering the standard library ({})...",
        compilers
            .iter()
            .filter_map(|c| c.file_name()?.to_str())
            .collect::<Vec<_>>()
            .join(", ")
    );
    let discovered = discover_all(&compilers)?;
    register_std(store, &discovered)?;
    eprintln!(
        "auto-registered the standard library (`{STD_ID}`) for {} compiler(s)",
        compilers.len()
    );
    Ok(())
}

fn existing_std_compilers(store: &LocalStore) -> Result<Vec<PathBuf>> {
    if !store.is_registered(STD_ID) {
        return Ok(Vec::new());
    }
    // add-std は登録を作り直すため、既存の tags.json からは認識コンパイラ集合しか使わない。
    // update と同じく、スキーマの合わない登録からでも集合を引き継げるよう検証せずに読む。
    Ok(Registration::load(&store.tags_json(STD_ID)?)?
        .compilers
        .unwrap_or_default())
}

fn discover_all(compilers: &[PathBuf]) -> Result<Vec<(PathBuf, Vec<PathBuf>)>> {
    compilers
        .iter()
        .map(|compiler| Ok((compiler.clone(), system_includes(compiler)?)))
        .collect()
}

/// `std` のディレクトリを作り直し、検出済みの `(コンパイラ, ルート群)` を 1 つのダミーツリーへ統合する。
///
/// 標準ライブラリは複数の dir (C++ 標準・コンパイラ組み込み・アーキ依存・C ライブラリ) に分散し、さらに
/// 複数コンパイラ分を混ぜるため、全てを 1 つのツリーへ集約する。相対パスが衝突しても復元する `#include`
/// は同一になるので無害。`tags.json` の `path` には代表として最初の dir を、`compilers` には認識集合を残す。
fn register_std(store: &LocalStore, discovered: &[(PathBuf, Vec<PathBuf>)]) -> Result<()> {
    let primary = discovered
        .iter()
        .find_map(|(_, roots)| roots.first())
        .cloned()
        .context("the system include paths are empty")?;
    recreate_library_dir(store, STD_ID)?;
    let dummy_dir = store.dummy_dir(STD_ID)?;
    for (compiler, roots) in discovered {
        eprintln!(
            "  generating dummies for the system includes of {}",
            compiler.display()
        );
        for root in roots {
            dummy::generate(root, &dummy_dir)?;
        }
    }
    Tags {
        path: primary,
        kind: TagsKind::Std {
            compilers: discovered.iter().map(|(c, _)| c.clone()).collect(),
        },
    }
    .save(&store.tags_json(STD_ID)?)
}

/// ライブラリの登録 (`$LOCAL/libraries/<id>` 一式) を削除する。
pub fn remove(store: &LocalStore, id: &str) -> Result<()> {
    let library_dir = store.library_dir(id)?;
    std::fs::remove_dir_all(&library_dir)
        .with_context(|| format!("failed to remove {}", library_dir.display()))
}

fn recreate_library_dir(store: &LocalStore, id: &str) -> Result<()> {
    let library_dir = store.library_dir(id)?;
    if library_dir.exists() {
        remove(store, id)?;
    }
    std::fs::create_dir_all(&library_dir)
        .with_context(|| format!("failed to create {}", library_dir.display()))
}

/// インクルードパスを絶対パスへ解決する。canonicalize は存在しないパスでエラーになるため、
/// 絶対パス化と存在確認を兼ねる。
///
/// `dunce::canonicalize` を使うのは Windows 対策。`std` の canonicalize は verbatim パス
/// (`\\?\C:\...`) を返し、これを `-I` に渡されたコンパイラはヘッダーを解決できない。
/// バンドル側の canonicalize も、突き合わせがずれないよう同じ理由で dunce に統一している。
pub fn resolve_source_root(path: &Path) -> Result<PathBuf> {
    dunce::canonicalize(path)
        .with_context(|| format!("failed to resolve include path {}", path.display()))
}

/// バンドル前に呼ぶ。`schema_version` が現行と合わない登録を、ライブラリ実体から黙って作り直す。
///
/// tags.json はライブラリ実体から再生成できるキャッシュであり、形式の不一致は risundle 側の都合
/// なので、ユーザーに `update` を要求せず自動で回復する (`std` の初回自動登録と同じ発想)。
/// 現行スキーマで読めるものは触らない。再生成に失敗した場合はエラーを返し、呼び出し側が案内する。
pub fn auto_migrate(store: &LocalStore) -> Result<()> {
    for id in store.library_ids()? {
        match Tags::load(&store.tags_json(&id)?) {
            Ok(_) => continue, // 現行スキーマ: 触らない
            Err(err) if err.downcast_ref::<SchemaMismatch>().is_some() => {} // 旧スキーマ: 下で作り直す
            Err(err) => return Err(err), // 破損など: 呼び出し側へ委ねる
        }
        eprintln!("migrating library `{id}` to the current tags format...");
        reregister(store, &id, None)?;
    }
    Ok(())
}

/// 既存の登録の中核 (`Registration`) からライブラリ実体を再走査し、登録を作り直す。
///
/// [`Registration`] はスキーマ検証をせず読むため、旧スキーマの登録からでも回復できる。std は
/// コンパイラ集合を、通常ライブラリは登録パス (または明示された `path`) を種にして作り直す。
#[expect(
    clippy::single_match_else,
    reason = "std と通常ライブラリで作り直し方が根本的に異なり、同格の分岐として match の対称性を保つ方が読みやすい"
)]
pub fn reregister(store: &LocalStore, id: &str, path: Option<&Path>) -> Result<()> {
    let reg = Registration::load(&store.tags_json(id)?)?;
    match reg.compilers {
        Some(compilers) => {
            if path.is_some() {
                bail!(
                    "a path cannot be specified for the standard library (it is auto-detected from the compiler)"
                );
            }
            let discovered = discover_all(&compilers)?;
            register_std(store, &discovered)?;
        }
        None => {
            // 保存済みパスも解決し直す: 実体が消えていれば登録を壊す前に失敗し (フェイルファスト)、
            // 経路が symlink 化していれば現在の正規形へ更新する。登録が常にその時点の正規形を保存
            // する、という add と同じ契約に揃える。
            let source_root = resolve_source_root(path.unwrap_or(&reg.path))?;
            register(store, id, &source_root)?;
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
    fn registers_non_std_library_with_files_dummy_and_hash() {
        let local = TempDir::new().unwrap();
        let store = store_in(&local);
        let source = source_with(&[("atcoder/modint.hpp", "struct modint {};")]);

        register(&store, "ac-library", source.path()).unwrap();

        assert!(store.is_registered("ac-library"));
        let tags = Tags::load(&store.tags_json("ac-library").unwrap()).unwrap();
        match tags.kind {
            TagsKind::Library { hash, files, .. } => {
                assert!(hash.starts_with("sha256:"));
                assert!(files["atcoder/modint.hpp"].contains(&"modint".to_owned()));
            }
            TagsKind::Std { .. } => panic!("非 std ライブラリは Library を持つべき"),
        }
        assert!(
            store
                .dummy_dir("ac-library")
                .unwrap()
                .join("atcoder/modint.hpp")
                .is_file()
        );
    }

    #[test]
    fn resolve_source_root_rejects_missing_path() {
        let local = TempDir::new().unwrap();
        assert!(resolve_source_root(&local.path().join("nonexistent")).is_err());
    }

    #[test]
    fn register_rejects_the_reserved_std_id() {
        // 受け口を経ない呼び出しでも、std 登録を Library 種別で上書きさせない。
        let local = TempDir::new().unwrap();
        let store = store_in(&local);
        let source = source_with(&[("vector", "// std header")]);

        assert!(register(&store, STD_ID, source.path()).is_err());
        assert!(!store.is_registered(STD_ID));
    }

    #[test]
    fn register_std_merges_multiple_compilers_into_one_dummy_tree() {
        let local = TempDir::new().unwrap();
        let store = store_in(&local);
        // 2 コンパイラ分のルートを模す。g++ 相当 (C++ 標準 + アーキ依存) と clang++ 相当 (組み込み)。
        let gcc_cpp = source_with(&[("vector", "// std"), ("bits/stdc++.h", "// all")]);
        let gcc_builtin = source_with(&[("immintrin.h", "// intrinsics")]);
        let clang_builtin = source_with(&[("arm_neon.h", "// neon")]);

        let discovered = vec![
            (
                PathBuf::from("/usr/bin/g++"),
                vec![
                    gcc_cpp.path().to_path_buf(),
                    gcc_builtin.path().to_path_buf(),
                ],
            ),
            (
                PathBuf::from("/usr/bin/clang++"),
                vec![clang_builtin.path().to_path_buf()],
            ),
        ];
        register_std(&store, &discovered).unwrap();

        let dummy = store.dummy_dir("std").unwrap();
        for file in ["vector", "bits/stdc++.h", "immintrin.h", "arm_neon.h"] {
            assert!(dummy.join(file).is_file(), "{file} がダミー化されていない");
        }

        let tags = Tags::load(&store.tags_json("std").unwrap()).unwrap();
        assert_eq!(
            tags.kind,
            TagsKind::Std {
                compilers: vec![
                    PathBuf::from("/usr/bin/g++"),
                    PathBuf::from("/usr/bin/clang++")
                ]
            }
        );
        // path は最初のコンパイラの最初のルート。
        assert_eq!(tags.path, gcc_cpp.path().to_path_buf());
    }

    #[test]
    fn reregister_migrates_tags_from_an_older_schema() {
        // 「スキーマ不一致は update で再生成」というエラー案内の受け皿として、reregister は
        // 旧スキーマの tags.json からでも登録パスを読み出して再登録できなければならない。
        let local = TempDir::new().unwrap();
        let store = store_in(&local);
        let source = source_with(&[("vec.hpp", "struct Vec {};")]);
        register(&store, "mylib", source.path()).unwrap();
        downgrade_schema(&store, "mylib");

        reregister(&store, "mylib", None).unwrap();

        assert!(
            Tags::load(&store.tags_json("mylib").unwrap()).is_ok(),
            "reregister 後は現行スキーマで読めるべき"
        );
    }

    #[test]
    fn auto_migrate_regenerates_outdated_libraries_and_leaves_current_ones() {
        let local = TempDir::new().unwrap();
        let store = store_in(&local);
        let source = source_with(&[("vec.hpp", "struct Vec {};")]);
        register(&store, "mylib", source.path()).unwrap();
        downgrade_schema(&store, "mylib");

        auto_migrate(&store).unwrap();
        assert!(
            Tags::load(&store.tags_json("mylib").unwrap()).is_ok(),
            "古い登録は移行されるべき"
        );

        // 既に現行スキーマなら再度呼んでも成功し、読める状態を保つ。
        auto_migrate(&store).unwrap();
        assert!(Tags::load(&store.tags_json("mylib").unwrap()).is_ok());
    }

    #[test]
    fn auto_migrate_propagates_non_schema_errors() {
        // スキーマ不一致 (回復可) と、破損など回復不能なエラーは区別する。後者は握り潰さず伝播する。
        let local = TempDir::new().unwrap();
        let store = store_in(&local);
        let source = source_with(&[("vec.hpp", "struct Vec {};")]);
        register(&store, "mylib", source.path()).unwrap();
        fs::write(store.tags_json("mylib").unwrap(), "{ not valid json").unwrap();

        assert!(auto_migrate(&store).is_err());
    }

    #[test]
    fn reregister_rejects_a_path_for_std() {
        // std の実体はコンパイラから検出するもので、パス指定は意味を持たないため弾く。
        let local = TempDir::new().unwrap();
        let store = store_in(&local);
        let roots = source_with(&[("vector", "// std header")]);
        register_std(
            &store,
            &[(
                PathBuf::from("/usr/bin/g++"),
                vec![roots.path().to_path_buf()],
            )],
        )
        .unwrap();

        let err = reregister(&store, STD_ID, Some(Path::new("/tmp")))
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("cannot be specified for the standard library"),
            "{err}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn reregister_rediscovers_std_from_the_stored_compilers() {
        use crate::library::testutil::fake_compiler_with_includes;

        let local = TempDir::new().unwrap();
        let store = store_in(&local);
        let scripts = TempDir::new().unwrap();
        let include = source_with(&[("vector", "// std header")]);
        let cc = fake_compiler_with_includes(scripts.path(), include.path());
        register_std(&store, &[(cc.clone(), vec![include.path().to_path_buf()])]).unwrap();

        // ダミーツリーを消し、path 無しの再登録が保存済みコンパイラから再検出することを確かめる。
        fs::remove_dir_all(store.dummy_dir(STD_ID).unwrap()).unwrap();
        reregister(&store, STD_ID, None).unwrap();

        assert!(store.dummy_dir(STD_ID).unwrap().join("vector").is_file());
        let tags = Tags::load(&store.tags_json(STD_ID).unwrap()).unwrap();
        assert_eq!(
            tags.kind,
            TagsKind::Std {
                compilers: vec![cc]
            }
        );
    }

    #[test]
    fn auto_setup_std_skips_when_already_registered() {
        let local = TempDir::new().unwrap();
        let store = store_in(&local);
        let roots = source_with(&[("vector", "// std header")]);
        let marker = PathBuf::from("/opt/previous-g++");
        register_std(
            &store,
            &[(marker.clone(), vec![roots.path().to_path_buf()])],
        )
        .unwrap();

        auto_setup_std(&store).unwrap();

        // 実コンパイラの有無に関わらず、既存の登録は上書きされない。
        let tags = Tags::load(&store.tags_json(STD_ID).unwrap()).unwrap();
        assert_eq!(
            tags.kind,
            TagsKind::Std {
                compilers: vec![marker]
            }
        );
    }

    #[test]
    fn auto_setup_std_registers_detected_compilers() {
        let local = TempDir::new().unwrap();
        let store = store_in(&local);
        if compiler::resolve(Path::new("g++")).is_err()
            && compiler::resolve(Path::new("clang++")).is_err()
        {
            return; // 候補コンパイラが無い環境ではスキップ
        }

        auto_setup_std(&store).unwrap();

        assert!(store.is_registered(STD_ID));
        let tags = Tags::load(&store.tags_json(STD_ID).unwrap()).unwrap();
        let TagsKind::Std { compilers } = tags.kind else {
            panic!("std は Std 種別で登録されるべき");
        };
        assert!(!compilers.is_empty());
    }

    #[test]
    fn add_std_keeps_the_compiler_set_from_an_older_schema() {
        let local = TempDir::new().unwrap();
        let store = store_in(&local);
        let Ok(g) = compiler::resolve(Path::new("g++")) else {
            return; // g++ が無い環境ではスキップ
        };
        add_std(&store, &g).unwrap();
        downgrade_schema(&store, "std");

        add_std(&store, &g).unwrap();
        assert!(Tags::load(&store.tags_json("std").unwrap()).is_ok());
    }
}
