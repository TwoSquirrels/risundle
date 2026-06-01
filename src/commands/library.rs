use std::path::Path;

use anyhow::bail;

use crate::cli::LibraryCommand;

pub fn run(command: LibraryCommand) -> anyhow::Result<()> {
    match command {
        LibraryCommand::Add { id, path } => add(&id, &path),
        LibraryCommand::Delete { id } => delete(&id),
        LibraryCommand::Update { id, path } => update(id.as_deref(), path.as_deref()),
        LibraryCommand::List => list(),
        LibraryCommand::Show { id } => show(&id),
    }
}

fn add(_id: &str, _path: &Path) -> anyhow::Result<()> {
    bail!("`library add` は未実装です");
}

fn delete(_id: &str) -> anyhow::Result<()> {
    bail!("`library delete` は未実装です");
}

fn update(_id: Option<&str>, _path: Option<&Path>) -> anyhow::Result<()> {
    bail!("`library update` は未実装です");
}

fn list() -> anyhow::Result<()> {
    bail!("`library list` は未実装です");
}

fn show(_id: &str) -> anyhow::Result<()> {
    bail!("`library show` は未実装です");
}
