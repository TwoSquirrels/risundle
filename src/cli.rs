use clap::{Parser, Subcommand};
use std::path::PathBuf;

// clap ではトップレベルの必須 positional と optional subcommand を共存させられないため、
// `library` 管理用の `LibraryCli` とはパーサを分け、エントリポイントで argv を振り分ける。

/// Tree-Shaking 機能付き競プロ用 C++ ソースバンドラー。
///
/// サブコマンドを指定しない場合は、指定された C++ ファイルのバンドルを実行する。
#[derive(Parser, Debug)]
#[command(
    name = "risundle",
    version,
    after_help = "ライブラリ管理は `risundle library --help` をご覧ください。"
)]
pub struct BundleArgs {
    /// 使用するコンパイラのパス
    #[arg(short, long)]
    pub compiler: Option<PathBuf>,

    /// Tree-Shaking 対象外として維持するライブラリ ID (複数指定可)
    #[arg(short, long = "keep")]
    pub keep: Vec<String>,

    /// オリジナルコードを先頭にコメントとして埋め込む
    #[arg(short, long)]
    pub embed: bool,

    /// ライブラリ更新のハッシュ検証をスキップする
    #[arg(short = 'n', long = "no-check")]
    pub no_check: bool,

    /// バンドル対象の C++ ソースファイル
    pub file: PathBuf,

    /// コンパイラにそのまま渡す追加オプション
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub options: Vec<String>,
}

// エントリポイントで argv の先頭を `risundle library` に差し替えて渡すため、
// help・usage 上のコマンド名が `risundle library` として表示される。

/// ライブラリの登録・管理を行う
#[derive(Parser, Debug)]
#[command(version)]
pub struct LibraryCli {
    #[command(subcommand)]
    pub command: LibraryCommand,
}

#[derive(Subcommand, Debug)]
pub enum LibraryCommand {
    /// ライブラリを登録する
    Add {
        /// ライブラリ ID
        id: String,
        /// インクルードパス
        path: PathBuf,
    },
    /// ライブラリの登録を削除する
    Delete {
        /// ライブラリ ID
        id: String,
    },
    /// ライブラリの更新を反映する (id 省略で全ライブラリ対象)
    Update {
        /// ライブラリ ID
        id: Option<String>,
        /// インクルードパス (省略時は登録済みのパスを使用)
        path: Option<PathBuf>,
    },
    /// 登録済みライブラリの一覧を表示する
    List,
    /// ライブラリの詳細を表示する
    Show {
        /// ライブラリ ID
        id: String,
    },
}
