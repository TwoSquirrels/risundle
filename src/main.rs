mod bundle;
mod cli;
mod commands;
mod compiler;
mod config;
mod fs;
mod library;
mod output;
mod update_check;

use clap::Parser;

use crate::cli::{Cli, Command};

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Some(Command::Library { command }) => commands::library::run(command),
        None => commands::bundle::run(cli.bundle),
    }
}
