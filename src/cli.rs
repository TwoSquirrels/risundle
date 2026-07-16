use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

/// A C++ source bundler with tree-shaking for competitive programming.
///
/// Bundles <FILE> and the registered libraries it includes into a single
/// file for submission, keeping only the headers it actually uses.
/// Register libraries beforehand with risundle library add.
#[derive(Parser, Debug)]
#[command(
    name = "risundle",
    version,
    propagate_version = true,
    subcommand_negates_reqs = true,
    args_conflicts_with_subcommands = true,
    override_usage = "risundle [OPTIONS] <FILE> [-- <COMPILER OPTIONS>...]\n       risundle library <COMMAND>",
    after_long_help = "Examples:\n  risundle library add mylib ~/cp/library\n  risundle main.cpp > submission.cpp"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    #[command(flatten)]
    pub bundle: BundleArgs,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Register and manage libraries
    ///
    /// Register a library once with add; bundling (risundle <FILE>) then
    /// recognizes its includes and tree-shakes them.
    // GNU Coding Standards に合わせ、-V はどの階層でも risundle 本体のバージョンを
    // 答える (propagate_version)。表示名の既定はハイフン結合 (risundle-library 等) で
    // 実在しないバイナリ名になってしまい、clap は display_name を伝播しないため、
    // 全サブコマンドで risundle に上書きする。
    #[command(display_name = "risundle")]
    Library {
        #[command(subcommand)]
        command: LibraryCommand,
    },
}

#[derive(Args, Debug)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "CLI フラグの列挙であり、bool の数は引数の数を映しているだけで状態機械の複雑さではない"
)]
pub struct BundleArgs {
    /// Path to the compiler to use [default: g++]
    ///
    /// Any GCC-compatible compiler works (g++, clang++, ...). A default can
    /// also be set in .risundlerc.toml.
    #[arg(short, long)]
    pub compiler: Option<PathBuf>,

    /// Also keep a library unexpanded, out of tree-shaking (can be repeated) [default: std]
    ///
    /// Takes the ID given at registration. Effective keep = (configured keep
    /// + every --keep) - every --no-keep.
    #[arg(short, long = "keep", value_name = "ID")]
    pub keep: Vec<String>,

    /// Stop keeping a library (can be repeated; beats --keep)
    ///
    /// Removes the ID from the configured keep and --keep, so the library is
    /// expanded and tree-shaken like any other.
    #[arg(long = "no-keep", value_name = "ID")]
    pub no_keep: Vec<String>,

    /// Embed the original source as a comment at the top
    #[arg(short, long, overrides_with = "no_embed")]
    pub embed: bool,

    /// Do not embed the original source (cancels a configured embed)
    #[arg(long, overrides_with = "embed")]
    pub no_embed: bool,

    /// Skip hash verification of library updates
    #[arg(short = 'n', long = "no-check")]
    pub no_check: bool,

    /// Expand the source without tree-shaking (fallback)
    ///
    /// Use when tree-shaking removed a definition the solution needs: every
    /// library except the kept ones stays fully expanded. Unlike --keep, the
    /// libraries are still expanded rather than left as #include.
    #[arg(long = "no-tree-shaking")]
    pub no_tree_shaking: bool,

    /// Ignore any .risundlerc.toml, behaving as if none exists
    ///
    /// The nearest .risundlerc.toml above <FILE> normally supplies defaults
    /// for the compiler, options, keep, and embed.
    #[arg(long = "no-config")]
    pub no_config: bool,

    /// C++ source file to bundle
    // subcommand_negates_reqs の都合で型は Option だが required は付いており、
    // バンドル経路 (subcommand 無し) では clap が存在を保証する。
    #[arg(required = true)]
    pub file: Option<PathBuf>,

    /// Extra options after -- passed straight to the compiler
    ///
    /// Appended to the options configured in .risundlerc.toml.
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

// 各 display_name の理由は Command::Library のコメントを参照。
#[derive(Subcommand, Debug)]
pub enum LibraryCommand {
    /// Register a library
    #[command(display_name = "risundle")]
    Add {
        /// Library ID, used to refer to the library later (e.g. in --keep)
        id: String,
        /// Include path: the library root directory
        path: PathBuf,
    },
    /// Register the standard library (std) from a compiler's system include paths
    ///
    /// Each call adds the compiler to the recognized set and merges its
    /// system include paths in, so multiple compilers can be used side by
    /// side.
    #[command(display_name = "risundle")]
    AddStd {
        /// Compiler whose system include paths to detect [default: g++]
        compiler: Option<PathBuf>,
    },
    /// Remove a library registration
    #[command(display_name = "risundle")]
    Delete {
        /// Library ID
        id: String,
    },
    /// Apply library changes (updates all libraries if ID is omitted)
    #[command(display_name = "risundle")]
    Update {
        /// Library ID
        id: Option<String>,
        /// Include path (uses the registered path if omitted)
        path: Option<PathBuf>,
    },
    /// List registered libraries
    #[command(display_name = "risundle")]
    List,
    /// Show library details
    #[command(display_name = "risundle")]
    Show {
        /// Library ID
        id: String,
        /// Also show the hash, per-file defined identifiers, and implementation target names
        #[arg(short, long)]
        verbose: bool,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_after_file_are_parsed_as_risundle_flags() {
        let cli = Cli::try_parse_from(["risundle", "main.cpp", "-e"]).unwrap();
        assert!(cli.bundle.embed);
        assert!(cli.bundle.options.is_empty());
    }

    #[test]
    fn double_dash_separates_compiler_options() {
        let cli =
            Cli::try_parse_from(["risundle", "main.cpp", "--", "-std=gnu++20", "-O2"]).unwrap();
        assert_eq!(cli.bundle.options, ["-std=gnu++20", "-O2"]);
    }

    #[test]
    fn hyphen_arguments_before_double_dash_are_rejected() {
        assert!(Cli::try_parse_from(["risundle", "main.cpp", "-O2"]).is_err());
    }

    #[test]
    fn embed_pair_resolves_to_the_last_flag() {
        let parse = |argv: &[&str]| {
            let argv = [&["risundle", "main.cpp"], argv].concat();
            Cli::try_parse_from(argv).unwrap().bundle.embed_override()
        };
        assert_eq!(parse(&[]), None);
        assert_eq!(parse(&["-e"]), Some(true));
        assert_eq!(parse(&["--no-embed"]), Some(false));
        // 後勝ち: 同時指定はコマンドライン上で後にある方が有効。
        assert_eq!(parse(&["--embed", "--no-embed"]), Some(false));
        assert_eq!(parse(&["--no-embed", "--embed"]), Some(true));
    }

    #[test]
    fn library_is_parsed_as_a_subcommand_not_a_file() {
        let cli = Cli::try_parse_from(["risundle", "library", "list"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Command::Library {
                command: LibraryCommand::List
            })
        ));
        assert_eq!(cli.bundle.file, None);
    }

    #[test]
    fn the_file_is_required_when_no_subcommand_is_given() {
        assert!(Cli::try_parse_from(["risundle"]).is_err());
    }

    #[test]
    fn bundle_flags_conflict_with_the_library_subcommand() {
        assert!(Cli::try_parse_from(["risundle", "-e", "library", "list"]).is_err());
    }

    // GNU Coding Standards 流: どの階層の、どんな打ちかけのコマンドでも、--version は
    // 他の引数 (必須引数の欠落も含む) に優先して risundle 本体のバージョンを答える。
    #[test]
    fn version_answers_as_risundle_at_every_level() {
        for argv in [
            vec!["risundle", "--version"],
            vec!["risundle", "main.cpp", "--version"],
            vec!["risundle", "library", "--version"],
            vec!["risundle", "library", "add", "--version"],
        ] {
            let err = Cli::try_parse_from(argv).unwrap_err();
            assert_eq!(err.kind(), clap::error::ErrorKind::DisplayVersion);
            assert!(err.to_string().starts_with("risundle "));
        }
    }
}
