mod cli;
mod commands;
mod config;
mod fs;
mod library;

use std::ffi::OsString;

use clap::Parser;

use crate::cli::{BundleArgs, LibraryCli};

fn main() -> anyhow::Result<()> {
    let argv: Vec<OsString> = std::env::args_os().collect();

    // 先頭引数が `library` のときだけ管理用パーサへ、それ以外は全てバンドル実行へ振り分ける。
    if argv.get(1).is_some_and(|arg| arg == "library") {
        // 先頭 2 要素 (コマンド名と `library`) を、usage 表示用の `risundle library` に差し替える。
        let args =
            std::iter::once(OsString::from("risundle library")).chain(argv.into_iter().skip(2));
        commands::library::run(LibraryCli::parse_from(args).command)
    } else {
        commands::bundle::run(BundleArgs::parse_from(argv))
    }
}
