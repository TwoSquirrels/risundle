//! バンドル実行のハンドラ。設定解決・コンパイラ起動などの IO をここに集約し、解析・置換の純粋
//! ロジックは [`crate::bundle`] の各モジュールに委ねる。全体の流れは:
//!
//! 1. 設定解決 (`.risundlerc.toml` + CLI マージ) とインベントリ読み込み・ハッシュ検証
//! 2. プリプロセス (`-E -C`) で linemarker 付きの展開結果を得る
//! 3. `<file>` 由来部分から識別子を検出 → 逆引きで依存ヘッダーを特定
//! 4. 依存ヘッダーに `-M` を実行 → 必要集合を得て、出力中の不要ヘッダーを判定
//! 5. 不要行の削除・ダミー pragma の復元・クレジット/埋め込みを施して出力

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::bundle::inventory::Inventory;
use crate::bundle::linemarker::{Line, Tracker};
use crate::bundle::{detect, prune, rewrite};
use crate::cli::BundleArgs;
use crate::commands::{compiler, library as cmd_library};
use crate::config;
use crate::fs::relpath;
use crate::library::local::LocalStore;

/// `std` として扱うライブラリ ID。未登録なら警告する。
const STD_ID: &str = "std";

pub fn run(args: BundleArgs) -> Result<()> {
    let store = LocalStore::discover()?;
    if let Err(err) = cmd_library::auto_setup_std(&store) {
        eprintln!("warning: failed to auto-register the standard library: {err:#}");
    }

    let settings = Settings::resolve(&args)?;
    let inventory = Inventory::load(&store, &settings.keep)?;
    warn_std_compiler(&settings.compiler, &inventory);
    if !args.no_check {
        inventory.verify()?;
    }

    let compiler_args = compiler_args(&settings, &inventory);
    let preprocessed = preprocess(&settings.compiler, &compiler_args, &args.file)?;

    let target = args
        .file
        .canonicalize()
        .with_context(|| format!("failed to resolve {}", args.file.display()))?;
    let unused = unused_origins(
        &settings,
        &inventory,
        &compiler_args,
        &preprocessed,
        &target,
    )?;
    let target_dir = target.parent();
    let bundled = rewrite::rewrite(
        &preprocessed,
        |origin| unused.contains(origin),
        |origin| display_origin(origin, &inventory, target_dir),
    );

    print!("{}", assemble_output(&args, &settings, &bundled)?);
    Ok(())
}

/// `#line` に出すファイル名を、ローカル絶対パスではなくライブラリ ID 基準の相対パスへ整える。
///
/// ライブラリ配下なら `<id>/<相対>`、入力ファイルと同じディレクトリ木の下ならそこからの相対、いずれ
/// でもなければファイル名のみへ落とす。提出物にホームディレクトリ名などローカルの絶対パスを残さない
/// のが目的。`<built-in>` 等 realpath 化できない出所はそのまま残す (デバッグの手掛かりとして無害)。
fn display_origin(origin: &str, inventory: &Inventory, target_dir: Option<&Path>) -> String {
    let Ok(canonical) = Path::new(origin).canonicalize() else {
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
    canonical
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| origin.to_owned())
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

/// `.risundlerc.toml` の設定に CLI オプションを重ねた実効設定。CLI で明示された項目が設定 (なければ
/// 組み込みデフォルト) を上書きする。
struct Settings {
    compiler: PathBuf,
    options: Vec<String>,
    keep: BTreeSet<String>,
    embed: bool,
}

impl Settings {
    fn resolve(args: &BundleArgs) -> Result<Self> {
        let config = config::resolve(&args.file)?;
        Ok(Self {
            compiler: args.compiler.clone().unwrap_or(config.compiler),
            options: if args.options.is_empty() {
                config.options
            } else {
                args.options.clone()
            },
            keep: if args.keep.is_empty() {
                config.keep.into_iter().collect()
            } else {
                args.keep.iter().cloned().collect()
            },
            embed: args.embed || config.embed,
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
    let dependency_headers = inventory.dependency_headers(&used, &present);
    let needed = needed_headers(&settings.compiler, compiler_args, &dependency_headers)?;

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
            .or_insert_with(|| Path::new(origin).canonicalize().ok())
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
        .filter_map(|path| path.canonicalize().ok())
        .collect())
}

/// クレジットと (任意で) オリジナルコードの埋め込みを先頭に付け、バンドル結果を組み立てる。
fn assemble_output(args: &BundleArgs, settings: &Settings, bundled: &str) -> Result<String> {
    let mut output = format!("// Bundled with risundle v{}\n", env!("CARGO_PKG_VERSION"));
    if settings.embed {
        let original = std::fs::read_to_string(&args.file)
            .with_context(|| format!("failed to read {}", args.file.display()))?;
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
            String::from_utf8_lossy(&output.stderr)
        );
    }
    String::from_utf8(output.stdout)
        .with_context(|| format!("the output of {what} could not be interpreted as UTF-8"))
}
