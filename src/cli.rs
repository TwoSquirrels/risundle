use clap::{Parser, Subcommand};
use std::path::PathBuf;

// clap ではトップレベルの必須 positional と optional subcommand を共存させられないため、
// `library` 管理用の `LibraryCli` とはパーサを分け、エントリポイントで argv を振り分ける。

/// A C++ source bundler with tree-shaking for competitive programming.
///
/// When no subcommand is given, bundles the specified C++ file.
#[derive(Parser, Debug)]
#[command(
    name = "risundle",
    version,
    after_help = "For library management, see `risundle library --help`."
)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "CLI フラグの列挙であり、bool の数は引数の数を映しているだけで状態機械の複雑さではない"
)]
pub struct BundleArgs {
    /// Path to the compiler to use
    #[arg(short, long)]
    pub compiler: Option<PathBuf>,

    /// Library ID to keep out of tree-shaking (can be repeated)
    #[arg(short, long = "keep")]
    pub keep: Vec<String>,

    /// Embed the original source as a comment at the top
    #[arg(short, long, overrides_with = "no_embed")]
    pub embed: bool,

    /// Do not embed the original source (cancels the config file)
    #[arg(long)]
    pub no_embed: bool,

    /// Skip hash verification of library updates
    #[arg(short = 'n', long = "no-check")]
    pub no_check: bool,

    /// Expand the source without tree-shaking
    #[arg(long = "no-tree-shaking")]
    pub no_tree_shaking: bool,

    /// C++ source file to bundle
    pub file: PathBuf,

    /// Extra options after `--` passed straight to the compiler
    #[arg(last = true, value_name = "COMPILER OPTIONS")]
    pub options: Vec<String>,
}

impl BundleArgs {
    /// `--embed` / `--no-embed` は後勝ちのペア。どちらも指定されなければ `None` を返し、
    /// 設定ファイル (なければ組み込み既定) に委ねる。
    pub fn embed_override(&self) -> Option<bool> {
        if self.embed {
            Some(true)
        } else if self.no_embed {
            Some(false)
        } else {
            None
        }
    }
}

// エントリポイントで argv の先頭を `risundle library` に差し替えて渡すため、
// help・usage 上のコマンド名が `risundle library` として表示される。

/// Register and manage libraries
#[derive(Parser, Debug)]
#[command(version)]
pub struct LibraryCli {
    #[command(subcommand)]
    pub command: LibraryCommand,
}

#[derive(Subcommand, Debug)]
pub enum LibraryCommand {
    /// Register a library
    Add {
        /// Library ID
        id: String,
        /// Include path
        path: PathBuf,
    },
    /// Register the standard library (`std`) (auto-detects the compiler's system include paths)
    AddStd {
        /// Compiler used to detect system include paths (defaults to g++)
        compiler: Option<PathBuf>,
    },
    /// Remove a library registration
    Delete {
        /// Library ID
        id: String,
    },
    /// Apply library updates (updates all libraries if id is omitted)
    Update {
        /// Library ID
        id: Option<String>,
        /// Include path (uses the registered path if omitted)
        path: Option<PathBuf>,
    },
    /// List registered libraries
    List,
    /// Show library details
    Show {
        /// Library ID
        id: String,
        /// Include the hash and per-file defined identifiers
        #[arg(short, long)]
        verbose: bool,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_after_file_are_parsed_as_risundle_flags() {
        let args = BundleArgs::try_parse_from(["risundle", "main.cpp", "-e"]).unwrap();
        assert!(args.embed);
        assert!(args.options.is_empty());
    }

    #[test]
    fn double_dash_separates_compiler_options() {
        let args =
            BundleArgs::try_parse_from(["risundle", "main.cpp", "--", "-std=gnu++20", "-O2"])
                .unwrap();
        assert_eq!(args.options, ["-std=gnu++20", "-O2"]);
    }

    #[test]
    fn hyphen_arguments_before_double_dash_are_rejected() {
        assert!(BundleArgs::try_parse_from(["risundle", "main.cpp", "-O2"]).is_err());
    }

    #[test]
    fn embed_pair_resolves_to_the_last_flag() {
        let parse = |argv: &[&str]| {
            let argv = [&["risundle", "main.cpp"], argv].concat();
            BundleArgs::try_parse_from(argv).unwrap().embed_override()
        };
        assert_eq!(parse(&[]), None);
        assert_eq!(parse(&["-e"]), Some(true));
        assert_eq!(parse(&["--no-embed"]), Some(false));
        // 後勝ち: 同時指定はコマンドライン上で後にある方が有効。
        assert_eq!(parse(&["--embed", "--no-embed"]), Some(false));
        assert_eq!(parse(&["--no-embed", "--embed"]), Some(true));
    }
}
