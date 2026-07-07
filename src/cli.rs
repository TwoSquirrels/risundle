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
pub struct BundleArgs {
    /// Path to the compiler to use
    #[arg(short, long)]
    pub compiler: Option<PathBuf>,

    /// Library ID to keep out of tree-shaking (can be repeated)
    #[arg(short, long = "keep")]
    pub keep: Vec<String>,

    /// Embed the original source as a comment at the top
    #[arg(short, long)]
    pub embed: bool,

    /// Skip hash verification of library updates
    #[arg(short = 'n', long = "no-check")]
    pub no_check: bool,

    /// Expand the source without tree-shaking
    #[arg(long = "no-tree-shaking")]
    pub no_tree_shaking: bool,

    /// C++ source file to bundle
    pub file: PathBuf,

    /// Extra options passed through to the compiler
    #[arg(last = true)]
    pub options: Vec<String>,
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
}
