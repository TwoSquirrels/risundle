//! バンドル実行のハンドラ。設定解決・コンパイラ起動などの IO をここに集約し、解析・置換の純粋
//! ロジックは [`crate::bundle`] の各モジュールに委ねる。全体の流れは:
//!
//! 1. 設定解決 (`.risundlerc.toml` + CLI マージ) とインベントリ読み込み・ハッシュ検証
//! 2. プリプロセス (`-E -C`) で linemarker 付きの展開結果を得る
//! 3. `<file>` 由来部分から識別子を検出 → 逆引きで依存ヘッダーを特定
//! 4. 依存ヘッダーに `-M` を実行 → 必要集合を得て、出力中の不要ヘッダーを判定
//! 5. 不要行の削除・ダミー pragma の復元・クレジット/埋め込みを施して出力
//!
//! `--no-tree-shaking` 時は識別子タグを一切使わないため、手順 1 のハッシュ検証と手順 3〜4 を
//! まるごとスキップする (不要ヘッダー無しとして扱う)。

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::bundle::inventory::Inventory;
use crate::bundle::linemarker::{Line, Tracker};
use crate::bundle::{detect, prune, rewrite};
use crate::cli::BundleArgs;
use crate::compiler;
use crate::config;
use crate::fs::relpath;
use crate::library::local::LocalStore;
use crate::library::registry::{self, STD_ID};

pub fn run(args: BundleArgs) -> Result<()> {
    let store = LocalStore::discover()?;
    if let Err(err) = registry::auto_setup_std(&store) {
        eprintln!("warning: failed to auto-register the standard library: {err:#}");
    }
    registry::auto_migrate(&store)?;

    // 設定へ吸収されない CLI 固有の値だけ、この場に残す。残りのフィールドは Settings::resolve へ
    // 所有権ごと渡し、設定とのマージで clone せずに済ませる。
    let embed = args.embed_override();
    let BundleArgs {
        compiler,
        keep,
        no_keep,
        no_check,
        no_tree_shaking,
        no_config,
        file,
        options,
        ..
    } = args;

    let std_removed_explicitly = no_keep.iter().any(|id| id == STD_ID);
    let settings = Settings::resolve(&file, no_config, compiler, options, keep, no_keep, embed)?;
    warn_std_not_kept(&settings.keep, std_removed_explicitly);
    let inventory = Inventory::load(&store, &settings.keep)?;
    warn_std_compiler(&settings.compiler, &inventory);
    if !no_check && !no_tree_shaking {
        inventory.verify()?;
    }

    let compiler_args = compiler_args(&settings, &inventory);
    let preprocessed = preprocess(&settings.compiler, &compiler_args, &file)?;

    // canonicalize は一律 dunce 版を使う (理由は registry::resolve_source_root を参照)。
    let target = dunce::canonicalize(&file)
        .with_context(|| format!("failed to resolve {}", file.display()))?;
    let unused = if no_tree_shaking {
        BTreeSet::new()
    } else {
        unused_origins(
            &settings,
            &inventory,
            &compiler_args,
            &preprocessed,
            &target,
        )?
    };
    let target_dir = target.parent();
    let bundled = rewrite::rewrite(
        &preprocessed,
        |origin| unused.contains(origin),
        |origin| display_origin(origin, &inventory, target_dir),
    );

    print!("{}", assemble_output(&file, &settings, &bundled)?);
    Ok(())
}

/// `#line` に出すファイル名を、ローカル絶対パスではなくライブラリ ID 基準の相対パスへ整える。
///
/// ライブラリ配下なら `<id>/<相対>`、入力ファイルと同じディレクトリ木の下ならそこからの相対、いずれ
/// でもなければファイル名のみへ落とす。提出物にホームディレクトリ名などローカルの絶対パスを残さない
/// のが目的。`<built-in>` 等 realpath 化できない出所はそのまま残す (デバッグの手掛かりとして無害)。
fn display_origin(origin: &str, inventory: &Inventory, target_dir: Option<&Path>) -> String {
    let Ok(canonical) = dunce::canonicalize(origin) else {
        return origin.to_owned();
    };
    if let Some(relative) = inventory.library_relative(&canonical) {
        return relative;
    }
    if let Some(relative) = target_dir
        .and_then(|dir| canonical.strip_prefix(dir).ok())
        .and_then(|rel| relpath::to_slash(rel).ok())
    {
        return relative;
    }
    canonical.file_name().map_or_else(
        || origin.to_owned(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// 実効 keep から std が外れていれば警告する。std の展開はほぼ常に事故で、巨大な出力が黙って
/// 生成され提出時のサイズ制限まで顕在化しないため、書き忘れ (設定ファイルの `keep` に std を
/// 挙げ損ねた等) は防護する。一方 CLI の `--no-keep std` は明示された意思なので警告しない。
fn warn_std_not_kept(keep: &BTreeSet<String>, removed_explicitly: bool) {
    if keep.contains(STD_ID) || removed_explicitly {
        return;
    }
    eprintln!(
        "warning: the standard library (`{STD_ID}`) is not kept and will be fully expanded; add \"{STD_ID}\" to `keep` in .risundlerc.toml, or pass `--no-keep {STD_ID}` to make the expansion explicit"
    );
}

/// std がバンドル対象のコンパイラ向けに登録されているかを確認し、外れていれば警告する。
///
/// std 未登録、または現在のコンパイラが std の認識集合に無い場合に `add-std` を促す。コンパイラは
/// 登録時と同じ規則で絶対パスへ正規化してから照合し、`g++` と `/usr/bin/g++` の表記揺れで誤警告
/// しないようにする。解決できないコンパイラは照合せず (バンドル本体が改めて報告する)。
fn warn_std_compiler(compiler: &Path, inventory: &Inventory) {
    let Some(recognized) = inventory.std_compilers() else {
        eprintln!(
            "warning: the standard library (`{STD_ID}`) is not registered; registering it with `risundle library add-std` is recommended"
        );
        return;
    };
    let Ok(resolved) = compiler::resolve(compiler) else {
        return;
    };
    if !recognized.contains(&resolved) {
        eprintln!(
            "warning: the standard library (`{STD_ID}`) is not registered for the current compiler ({}); consider `risundle library add-std {}`",
            resolved.display(),
            compiler.display()
        );
    }
}

/// `.risundlerc.toml` の設定に CLI オプションを重ねた実効設定。重ね方は項目の型ごとに決まる:
/// スカラー (`compiler`) と bool (`embed`) は CLI 明示が設定を上書きし、集合 (`keep`) は
/// 設定へ加算した上で `--no-keep` を除き、順序付きリスト (`options`) は設定へ追記する (#24)。
struct Settings {
    compiler: PathBuf,
    options: Vec<String>,
    keep: BTreeSet<String>,
    embed: bool,
}

impl Settings {
    /// CLI で明示された値を消費して設定と重ね合わせる。`file` は設定ファイルの探索起点として
    /// 借用するだけで、呼び出し側が引き続き所有する。
    fn resolve(
        file: &Path,
        no_config: bool,
        compiler: Option<PathBuf>,
        options: Vec<String>,
        keep: Vec<String>,
        no_keep: Vec<String>,
        embed: Option<bool>,
    ) -> Result<Self> {
        // --no-config は「設定ファイルが 1 つも見つからない環境」と完全に同一の挙動と定義する。
        // これにより、CLI だけで組み込み既定から実効設定を組み立て直せることが常に保証される。
        let config = if no_config {
            config::Config::default()
        } else {
            config::resolve(file)?
        };
        // 実効 keep = (config の keep ∪ --keep) − --no-keep。同じ ID が両方にあれば順序に依らず
        // --no-keep が勝つ (誤 keep はジャッジで解決できない #include を残す硬い失敗、誤展開は
        // ファイルが膨らむだけの柔らかい失敗なので、衝突は展開側へ倒す)。
        let no_keep: BTreeSet<String> = no_keep.into_iter().collect();
        Ok(Self {
            compiler: compiler.unwrap_or(config.compiler),
            // 実効 options = config の options + CLI の options。上書きの意味論はコンパイラの
            // 後勝ち (-std 等) や -U に委ね、risundle 側では重複や矛盾を解釈しない。
            options: config.options.into_iter().chain(options).collect(),
            keep: config
                .keep
                .into_iter()
                .chain(keep)
                .filter(|id| !no_keep.contains(id))
                .collect(),
            embed: embed.unwrap_or(config.embed),
        })
    }
}

/// コンパイラへ渡す共通オプション (`$options` + `-nostdinc` + `-I...`)。プリプロセスと `-M` で共有する。
fn compiler_args(settings: &Settings, inventory: &Inventory) -> Vec<String> {
    let mut args = settings.options.clone();
    if inventory.uses_nostdinc() {
        args.push("-nostdinc".to_owned());
    }
    args.extend(inventory.include_flags());
    args
}

/// 出力中の不要ヘッダーを、その linemarker パス文字列の集合として特定する。
fn unused_origins(
    settings: &Settings,
    inventory: &Inventory,
    compiler_args: &[String],
    preprocessed: &str,
    target: &Path,
) -> Result<BTreeSet<String>> {
    let (target_code, pruneable) = scan_origins(preprocessed, target, inventory);

    // 出力に現れた維持指定外ライブラリのファイル群。これが逆引きの母集合かつ不要判定の候補になる。
    let present: BTreeSet<PathBuf> = pruneable.values().cloned().collect();
    let used = detect::identifiers(&target_code);
    let mut dependency_headers = inventory.dependency_headers(&used, &present);

    // 必要ファイルが定義する型の実装ファイル (演算子オーバーロード等、識別子に現れない依存) を
    // 逆引きで加え、増えなくなるまで `-M` の必要集合と交互に更新する。dependency_headers は単調
    // 増加で present に有界なので必ず停止する。
    let needed = loop {
        let needed = needed_headers(&settings.compiler, compiler_args, &dependency_headers)?;
        let implementations = inventory.implementation_files(&needed, &present);
        if implementations.is_subset(&dependency_headers) {
            break needed;
        }
        dependency_headers.extend(implementations);
    };

    let unused = prune::unused_headers(&present, &needed);

    // 不要と判定した realpath を、rewrite が突き合わせる linemarker のパス文字列へ戻す。
    Ok(pruneable
        .into_iter()
        .filter(|(_, canonical)| unused.contains(canonical))
        .map(|(origin, _)| origin)
        .collect())
}

/// プリプロセス出力を 1 度走査し、`<file>` 由来コードと、削除候補ヘッダー (出所文字列 → realpath) を
/// 集める。realpath 化は出所文字列ごとに 1 度だけ行う。
fn scan_origins(
    preprocessed: &str,
    target: &Path,
    inventory: &Inventory,
) -> (String, HashMap<String, PathBuf>) {
    let mut tracker = Tracker::new();
    let mut canonical_cache: HashMap<String, Option<PathBuf>> = HashMap::new();
    let mut target_code = String::new();
    let mut pruneable = HashMap::new();

    for line in preprocessed.lines() {
        let Line::Code { file: Some(origin) } = tracker.observe(line) else {
            continue;
        };
        let canonical = canonical_cache
            .entry(origin.to_owned())
            .or_insert_with(|| dunce::canonicalize(origin).ok())
            .clone();
        let Some(canonical) = canonical else {
            continue; // <built-in> など実在しない出所は無視
        };
        if canonical == *target {
            target_code.push_str(line);
            target_code.push('\n');
        } else if inventory.is_pruneable(&canonical) {
            pruneable.insert(origin.to_owned(), canonical);
        }
    }
    (target_code, pruneable)
}

/// 依存ヘッダーに `-M` を実行し、必要集合 (推移的に取り込まれる全ヘッダー) を realpath で返す。
/// 依存ヘッダーが無ければ必要集合は空 (= 全候補が不要)。
fn needed_headers(
    compiler: &Path,
    compiler_args: &[String],
    dependency_headers: &BTreeSet<PathBuf>,
) -> Result<BTreeSet<PathBuf>> {
    if dependency_headers.is_empty() {
        return Ok(BTreeSet::new());
    }
    let make_output = make_dependencies(compiler, compiler_args, dependency_headers)?;
    Ok(prune::parse_prerequisites(&make_output)
        .iter()
        .filter_map(|path| dunce::canonicalize(path).ok())
        .collect())
}

/// クレジットと (任意で) オリジナルコードの埋め込みを先頭に付け、バンドル結果を組み立てる。
fn assemble_output(file: &Path, settings: &Settings, bundled: &str) -> Result<String> {
    let mut output = format!("// Bundled with risundle v{}\n", env!("CARGO_PKG_VERSION"));
    if settings.embed {
        let original = std::fs::read_to_string(file)
            .with_context(|| format!("failed to read {}", file.display()))?;
        output.push_str("//\n// --- original source ---\n");
        for line in original.lines() {
            output.push_str("// ");
            output.push_str(line);
            output.push('\n');
        }
        output.push_str("// --- end original source ---\n");
    }
    output.push_str(bundled);
    Ok(output)
}

/// `$compiler $args -x c++ -E -C <file>` でプリプロセス結果を得る。
fn preprocess(compiler: &Path, compiler_args: &[String], file: &Path) -> Result<String> {
    let mut command = Command::new(compiler);
    command
        .args(compiler_args)
        .args(["-x", "c++", "-E", "-C"])
        .arg(file);
    run_capturing(command, compiler, "preprocessing")
}

/// `$compiler $args -x c++ -M <headers...>` で依存ヘッダーの推移閉包を make ルールとして得る。
fn make_dependencies(
    compiler: &Path,
    compiler_args: &[String],
    headers: &BTreeSet<PathBuf>,
) -> Result<String> {
    let mut command = Command::new(compiler);
    command
        .args(compiler_args)
        .args(["-x", "c++", "-M"])
        .args(headers);
    run_capturing(command, compiler, "dependency resolution")
}

/// コンパイラを起動し、標準出力を文字列で返す。失敗時は標準エラーを添えてエラーにする。
fn run_capturing(mut command: Command, compiler: &Path, what: &str) -> Result<String> {
    let output = command.output().with_context(|| {
        format!(
            "failed to launch compiler {} for {what}",
            compiler.display()
        )
    })?;
    if !output.status.success() {
        bail!(
            "{what} failed:\n{}",
            String::from_utf8_lossy(&output.stderr).trim_end()
        );
    }
    String::from_utf8(output.stdout)
        .with_context(|| format!("the output of {what} could not be interpreted as UTF-8"))
}

#[cfg(test)]
mod tests {
    use super::*;

    use tempfile::TempDir;

    use crate::config::Config;
    use crate::library::local::LocalStore;
    use crate::library::testutil::{store_in, write_std_registration};

    fn empty_inventory(store: &LocalStore) -> Inventory {
        Inventory::load(store, &BTreeSet::new()).unwrap()
    }

    /// `compiler::resolve` が通る「コンパイラ」(中身は空ファイル) を作る。警告の照合はパスの
    /// 突き合わせだけで、起動はしない。
    fn stub_compiler(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, "").unwrap();
        path
    }

    #[test]
    fn warn_std_compiler_handles_every_registration_state() {
        // 警告は標準エラーへの出力のみで戻り値を持たないため、各経路が落ちずに通ることを確かめる
        // (経路の選択自体は resolve と集合照合のロジックで、ここで実行される)。
        let local = TempDir::new().unwrap();
        let store = store_in(&local);
        let bins = TempDir::new().unwrap();
        let known = stub_compiler(bins.path(), "g++");
        let unknown = stub_compiler(bins.path(), "clang++");

        // std 未登録 → 登録を促す警告。
        warn_std_compiler(&known, &empty_inventory(&store));

        write_std_registration(&store, vec![std::path::absolute(&known).unwrap()]);
        let inventory = empty_inventory(&store);
        // 認識済みコンパイラ → 警告なし。
        warn_std_compiler(&known, &inventory);
        // 認識外コンパイラ → add-std を促す警告。
        warn_std_compiler(&unknown, &inventory);
        // 解決できないコンパイラ → 照合せず戻る (バンドル本体が改めて報告する)。
        warn_std_compiler(Path::new("no/such/compiler"), &inventory);
    }

    #[test]
    fn display_origin_falls_back_to_the_raw_origin() {
        let local = TempDir::new().unwrap();
        let store = store_in(&local);
        let inventory = empty_inventory(&store);

        // "/" は realpath 化できるがファイル名を持たない → 出所文字列のまま。
        assert_eq!(display_origin("/", &inventory, None), "/");
        // 実在しない出所 (<built-in> など) もそのまま残す。
        assert_eq!(display_origin("<built-in>", &inventory, None), "<built-in>");
    }

    #[test]
    fn needed_headers_without_dependencies_skips_the_compiler() {
        // 依存ヘッダーが無ければ空集合 (= 全候補が不要)。コンパイラは起動されないので、
        // 存在しないパスを渡しても成功することがその証明になる。
        let needed =
            needed_headers(Path::new("compiler-must-not-run"), &[], &BTreeSet::new()).unwrap();
        assert!(needed.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn preprocess_reports_compiler_failure_with_its_stderr() {
        use crate::library::testutil::fake_compiler;

        let scripts = TempDir::new().unwrap();
        let cc = fake_compiler(scripts.path(), "echo 'syntax error' >&2\nexit 1");

        let err = preprocess(&cc, &[], Path::new("main.cpp"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("preprocessing failed"), "{err}");
        assert!(
            err.contains("syntax error"),
            "原因特定のためコンパイラの標準エラーを含めるべき: {err}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn run_capturing_rejects_non_utf8_output() {
        use crate::library::testutil::fake_compiler;

        let scripts = TempDir::new().unwrap();
        let cc = fake_compiler(scripts.path(), "printf '\\377'");

        let err = preprocess(&cc, &[], Path::new("main.cpp"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("could not be interpreted as UTF-8"), "{err}");
    }

    #[test]
    fn settings_prefer_cli_values_and_fall_back_to_defaults() {
        let temp = TempDir::new().unwrap();
        let file = temp.path().join("main.cpp");
        std::fs::write(&file, "int main() {}").unwrap();

        // CLI で明示された値が勝つ (keep は設定への加算、options は設定への追記)。
        let cli = Settings::resolve(
            &file,
            false,
            Some(PathBuf::from("my-g++")),
            vec!["-O0".to_owned()],
            vec!["ac-library".to_owned()],
            vec![],
            Some(true),
        )
        .unwrap();
        assert_eq!(cli.compiler, PathBuf::from("my-g++"));
        let mut expected_options = Config::default().options;
        expected_options.push("-O0".to_owned());
        // 追記なので -std=gnu++17 等の既定は生き残り、-O0 は既定の -O2 に後勝ちする。
        assert_eq!(cli.options, expected_options);
        assert!(cli.keep.contains("ac-library"));
        assert!(cli.keep.contains("std"), "-k は既定の std を消さない");
        assert!(cli.embed);

        // CLI 省略時は設定 (.risundlerc.toml が無いここでは組み込みデフォルト) が生きる。
        let defaults = Settings::resolve(&file, false, None, vec![], vec![], vec![], None).unwrap();
        let expected = Config::default();
        assert_eq!(defaults.compiler, expected.compiler);
        assert_eq!(defaults.options, expected.options);
        assert_eq!(
            defaults.keep,
            expected.keep.into_iter().collect::<BTreeSet<_>>()
        );
        assert!(!defaults.embed);
    }

    #[test]
    fn no_embed_cancels_the_config_file() {
        // 設定ファイルの embed = true を、CLI の明示 (--no-embed = Some(false)) が打ち消せる。
        let temp = TempDir::new().unwrap();
        std::fs::write(
            temp.path().join(".risundlerc.toml"),
            "[bundle]\nembed = true\n",
        )
        .unwrap();
        let file = temp.path().join("main.cpp");
        std::fs::write(&file, "int main() {}").unwrap();

        let from_config =
            Settings::resolve(&file, false, None, vec![], vec![], vec![], None).unwrap();
        assert!(from_config.embed);
        let cancelled =
            Settings::resolve(&file, false, None, vec![], vec![], vec![], Some(false)).unwrap();
        assert!(!cancelled.embed);
    }

    #[test]
    fn no_config_behaves_as_if_no_config_file_exists() {
        // --no-config は「設定ファイルが 1 つも見つからない環境」と同一挙動、が仕様の定義。
        let temp = TempDir::new().unwrap();
        std::fs::write(
            temp.path().join(".risundlerc.toml"),
            "[compiler]\npath = \"clang++\"\noptions = [\"-I/secret\"]\n",
        )
        .unwrap();
        let file = temp.path().join("main.cpp");
        std::fs::write(&file, "int main() {}").unwrap();

        let isolated = Settings::resolve(&file, true, None, vec![], vec![], vec![], None).unwrap();
        let expected = Config::default();
        assert_eq!(isolated.compiler, expected.compiler);
        assert_eq!(isolated.options, expected.options);
        assert_eq!(
            isolated.keep,
            expected.keep.into_iter().collect::<BTreeSet<_>>()
        );
        assert!(!isolated.embed);
    }

    #[test]
    fn warn_std_not_kept_covers_absence_and_explicit_removal() {
        // 警告は標準エラーへの出力のみで戻り値を持たないため、各経路が落ちずに通ることを確かめる。
        let kept: BTreeSet<String> = ["std".to_owned()].into();
        // std が keep にある → 警告なし。
        warn_std_not_kept(&kept, false);
        // std が無い (書き忘れの可能性) → 警告。
        warn_std_not_kept(&BTreeSet::new(), false);
        // --no-keep std の明示 → 意図を尊重して警告なし。
        warn_std_not_kept(&BTreeSet::new(), true);
    }

    #[test]
    fn no_keep_subtracts_from_the_keep_set() {
        let temp = TempDir::new().unwrap();
        let file = temp.path().join("main.cpp");
        std::fs::write(&file, "int main() {}").unwrap();
        let resolve = |keep: &[&str], no_keep: &[&str]| {
            let owned = |ids: &[&str]| ids.iter().map(|&id| (*id).to_owned()).collect();
            Settings::resolve(
                &file,
                false,
                None,
                vec![],
                owned(keep),
                owned(no_keep),
                None,
            )
            .unwrap()
            .keep
        };

        // --no-keep std で、CLI から keep を空にできる (可逆性の回復)。
        assert!(resolve(&[], &["std"]).is_empty());
        // 同じ ID が両方にあれば、順序に依らず --no-keep が勝つ。
        assert!(!resolve(&["std"], &["std"]).contains("std"));
        // 他の ID には影響しない。
        assert!(resolve(&["ac-library"], &["std"]).contains("ac-library"));
    }
}
