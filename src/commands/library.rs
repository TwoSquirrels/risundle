use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

use crate::cli::LibraryCommand;
use crate::commands::compiler::resolve as resolve_compiler;
use crate::config::Config;
use crate::library::local::LocalStore;
use crate::library::tags::{Tags, TagsKind};
use crate::library::{dummy, hash, identifiers};

/// `std` として扱うライブラリ ID。識別子情報を持たず、更新検知の対象外とする。
const STD_ID: &str = "std";

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
    let source_root = resolve_source_root(path)?;

    eprintln!("registering library `{id}`...");
    register_library(store, id, &source_root)?;

    println!("registered library `{id}`");
    Ok(())
}

/// `std` に 1 つのコンパイラを加える。既に登録済みなら、その認識集合へ追加して統合ツリーを作り直す。
///
/// 単一のグローバルコンパイラを握るのではなく「認識しているコンパイラの集合」を育てる方針。集合の全
/// コンパイラのシステム include パスを 1 つのダミーツリーへ統合するため、どのコンパイラでバンドルしても
/// 解決でき、背反が起きない。コンパイラは絶対パスへ正規化して表記揺れ (`g++` と `/usr/bin/g++`) を防ぐ。
fn add_std(store: &LocalStore, compiler: Option<&Path>) -> Result<()> {
    let requested = compiler
        .map(Path::to_path_buf)
        .unwrap_or_else(|| Config::default().compiler);
    let resolved = resolve_compiler(&requested)?;

    let mut compilers = existing_std_compilers(store)?;
    if !compilers.contains(&resolved) {
        compilers.push(resolved);
    }

    eprintln!("registering the standard library...");
    let discovered = discover_all(&compilers)?;
    register_std(store, &discovered)?;

    println!(
        "registered the standard library (`{STD_ID}`) for {} compiler(s)",
        compilers.len()
    );
    Ok(())
}

const AUTO_DETECT_CANDIDATES: &[&str] = &["g++", "clang++"];

/// std が未登録なら、PATH 内の候補コンパイラ (g++/clang++) を自動検出して登録する。
///
/// バンドル実行の初回セットアップ用。コンパイラが 1 つも見つからなければ何もしない
/// (後段の `warn_std_compiler` が案内する)。既に登録済みならスキップする。
pub fn auto_setup_std(store: &LocalStore) -> Result<()> {
    if store.is_registered(STD_ID) {
        return Ok(());
    }
    let compilers: Vec<PathBuf> = AUTO_DETECT_CANDIDATES
        .iter()
        .filter_map(|name| resolve_compiler(Path::new(name)).ok())
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
    match Tags::load(&store.tags_json(STD_ID))?.kind {
        TagsKind::Std { compilers } => Ok(compilers),
        TagsKind::Library { .. } => Ok(Vec::new()),
    }
}

fn discover_all(compilers: &[PathBuf]) -> Result<Vec<(PathBuf, Vec<PathBuf>)>> {
    compilers
        .iter()
        .map(|compiler| Ok((compiler.clone(), discover_system_includes(compiler)?)))
        .collect()
}

/// 通常ライブラリのディレクトリを作り直し、ダミー・`tags.json` (hash + files) を生成する。
///
/// `source_root` は解決済みの絶対パスを前提とする (`tags.json` にそのまま保存するため)。既存の
/// ディレクトリは丸ごと作り直すので、登録失敗で残った不完全な状態や、更新前の古い内容を引きずらない。
fn register_library(store: &LocalStore, id: &str, source_root: &Path) -> Result<()> {
    recreate_library_dir(store, id)?;
    dummy::generate(source_root, &store.dummy_dir(id))?;

    // 識別子抽出はファイル数に比例して時間がかかるため、処理中のファイル名を逐次表示する。
    let files = identifiers::enumerate(source_root, |relative| eprintln!("  {relative}"))?;
    let hash = hash::aggregate(source_root)?;
    Tags {
        path: source_root.to_path_buf(),
        kind: TagsKind::Library {
            hash,
            files,
            implements: Default::default(),
        },
    }
    .save(&store.tags_json(id))
}

/// `std` のディレクトリを作り直し、検出済みの `(コンパイラ, ルート群)` を 1 つのダミーツリーへ統合する。
///
/// 標準ライブラリは複数の dir (C++ 標準・コンパイラ組み込み・アーキ依存・C ライブラリ) に分散し、さらに
/// 複数コンパイラ分を混ぜるため、全てを 1 つのツリーへ集約する。相対パスが衝突しても復元する `#include`
/// は同一になるので無害。`tags.json` の `path` には代表として最初の dir を、`compilers` には認識集合を残す。
/// ルート検出 (コンパイラ起動) は呼び出し側が行い、ここは純粋に書き込みのみを担う。
fn register_std(store: &LocalStore, discovered: &[(PathBuf, Vec<PathBuf>)]) -> Result<()> {
    let primary = discovered
        .iter()
        .flat_map(|(_, roots)| roots.first())
        .next()
        .cloned()
        .context("the system include paths are empty")?;
    recreate_library_dir(store, STD_ID)?;
    let dummy_dir = store.dummy_dir(STD_ID);
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
    .save(&store.tags_json(STD_ID))
}

fn recreate_library_dir(store: &LocalStore, id: &str) -> Result<()> {
    let library_dir = store.library_dir(id);
    if library_dir.exists() {
        std::fs::remove_dir_all(&library_dir)
            .with_context(|| format!("failed to remove {}", library_dir.display()))?;
    }
    std::fs::create_dir_all(&library_dir)
        .with_context(|| format!("failed to create {}", library_dir.display()))
}

/// `-v` 付きプリプロセスの標準エラーに出る探索リストを解析する。`CPATH` 等の環境変数は探索パスを
/// 汚染する (ユーザーのライブラリが紛れる) ため取り除き、コンパイラ本来のシステム dir だけを得る。
fn discover_system_includes(compiler: &Path) -> Result<Vec<PathBuf>> {
    let output = Command::new(compiler)
        .args(["-E", "-x", "c++", "-v", "-"])
        .env_remove("CPATH")
        .env_remove("C_INCLUDE_PATH")
        .env_remove("CPLUS_INCLUDE_PATH")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .with_context(|| format!("failed to launch compiler {}", compiler.display()))?;
    if !output.status.success() {
        bail!(
            "failed to detect the system include paths of compiler {}:\n{}",
            compiler.display(),
            String::from_utf8_lossy(&output.stderr).trim_end()
        );
    }
    let roots = parse_search_dirs(&String::from_utf8_lossy(&output.stderr));
    if roots.is_empty() {
        bail!(
            "could not detect any system include paths for compiler {}",
            compiler.display()
        );
    }
    Ok(roots)
}

/// `-v` 出力から `#include <...> search starts here:` 〜 `End of search list.` の dir 一覧を取り出す。
/// 実在するディレクトリのみを realpath 化して返す。
fn parse_search_dirs(verbose_output: &str) -> Vec<PathBuf> {
    let mut lines = verbose_output.lines();
    lines
        .by_ref()
        .find(|line| line.contains("#include <...> search starts here:"));
    lines
        .take_while(|line| !line.contains("End of search list."))
        .filter_map(|line| {
            let dir = PathBuf::from(line.trim());
            dir.is_dir().then(|| dir.canonicalize().ok()).flatten()
        })
        .collect()
}

/// インクルードパスを絶対パスへ解決する。`canonicalize` は存在しないパスでエラーになるため、
/// 絶対パス化と存在確認を兼ねる。
fn resolve_source_root(path: &Path) -> Result<PathBuf> {
    path.canonicalize()
        .with_context(|| format!("failed to resolve include path {}", path.display()))
}

/// ライブラリ ID がパス要素として安全か検証する。
///
/// ID はそのまま `$LOCAL/libraries/<id>` のディレクトリ名になるため、空・`.`/`..`・パス区切りを
/// 含む ID を許すと意図しない場所を読み書きしてしまう。フェイルファストで早期に弾く。
fn validate_id(id: &str) -> Result<()> {
    if id.is_empty() || id == "." || id == ".." || id.contains('/') || id.contains('\\') {
        bail!(
            "library ID `{id}` is not allowed (empty, `.`/`..`, or IDs containing path separators are rejected)"
        );
    }
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
    let library_dir = store.library_dir(id);
    std::fs::remove_dir_all(&library_dir)
        .with_context(|| format!("failed to remove {}", library_dir.display()))?;

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
    let tags = Tags::load(&store.tags_json(id))?;

    eprintln!("updating library `{id}`...");
    match tags.kind {
        TagsKind::Std { compilers } => {
            if path.is_some() {
                bail!(
                    "a path cannot be specified for the standard library (it is auto-detected from the compiler)"
                );
            }
            let discovered = discover_all(&compilers)?;
            register_std(store, &discovered)?;
        }
        TagsKind::Library { .. } => {
            let source_root = match path {
                Some(path) => resolve_source_root(path)?,
                None => tags.path,
            };
            register_library(store, id, &source_root)?;
        }
    }

    println!("updated library `{id}`");
    Ok(())
}

fn list(store: &LocalStore) -> Result<()> {
    let ids = store.library_ids()?;
    if ids.is_empty() {
        println!("no libraries are registered");
        return Ok(());
    }
    // 種別を足しつつタブ区切りを保ち、grep/awk などでのパイプ処理を妨げない。
    for id in ids {
        let tags = Tags::load(&store.tags_json(&id))?;
        println!("{id}\t{}\t{}", kind_label(&tags.kind), tags.path.display());
    }
    Ok(())
}

fn kind_label(kind: &TagsKind) -> &'static str {
    match kind {
        TagsKind::Std { .. } => "std",
        TagsKind::Library { .. } => "library",
    }
}

/// `show` の 1 項目を、ラベル幅を揃えて出力する。最長ラベル `Compilers` に合わせる。
fn show_field(label: &str, value: &str) {
    println!("{label:<9} {value}");
}

fn show(store: &LocalStore, id: &str, verbose: bool) -> Result<()> {
    validate_id(id)?;
    ensure_registered(store, id)?;
    let tags = Tags::load(&store.tags_json(id))?;
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
        TagsKind::Library { hash, files, .. } => {
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
            TagsKind::Library { hash, files, .. } => {
                assert!(hash.starts_with("sha256:"));
                assert!(files["atcoder/modint.hpp"].contains(&"modint".to_owned()));
            }
            TagsKind::Std { .. } => panic!("非 std ライブラリは Library を持つべき"),
        }
        assert!(
            store
                .dummy_dir("ac-library")
                .join("atcoder/modint.hpp")
                .is_file()
        );
    }

    #[test]
    fn add_rejects_std_id() {
        // std は専用の add-std で登録する。汎用 add は弾く。
        let local = TempDir::new().unwrap();
        let store = store_in(&local);
        let source = source_with(&[("vector", "// std header")]);

        assert!(add(&store, "std", source.path()).is_err());
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

        let dummy = store.dummy_dir("std");
        for file in ["vector", "bits/stdc++.h", "immintrin.h", "arm_neon.h"] {
            assert!(dummy.join(file).is_file(), "{file} がダミー化されていない");
        }

        let tags = Tags::load(&store.tags_json("std")).unwrap();
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
    fn parses_search_dirs_between_markers() {
        let verbose = "ignored preamble\n\
            #include \"...\" search starts here:\n\
            #include <...> search starts here:\n \
            /nonexistent/should/skip\n \
            .\n\
            End of search list.\n\
            trailing junk\n";
        // 実在する dir のみ realpath 化される。"." はカレントなので拾われる。
        // 期待値も同じく canonicalize する: Windows の verbatim パス (`\\?\`) や macOS の
        // symlink (/tmp→/private/tmp) で表記が分岐するため、関数と同じ正規化を通して比較する。
        let dirs = parse_search_dirs(verbose);
        assert_eq!(dirs, vec![Path::new(".").canonicalize().unwrap()]);
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
