use std::path::Path;

use anyhow::bail;

use crate::cli::LibraryCommand;
use crate::local::LocalStore;

pub fn run(command: LibraryCommand) -> anyhow::Result<()> {
    let store = LocalStore::discover()?;
    match command {
        LibraryCommand::Add { id, path } => add(&store, &id, &path),
        LibraryCommand::Delete { id } => delete(&store, &id),
        LibraryCommand::Update { id, path } => update(&store, id.as_deref(), path.as_deref()),
        LibraryCommand::List => list(&store),
        LibraryCommand::Show { id } => show(&store, &id),
    }
}

fn add(_store: &LocalStore, _id: &str, _path: &Path) -> anyhow::Result<()> {
    bail!("`library add` は未実装です");
}

fn delete(_store: &LocalStore, _id: &str) -> anyhow::Result<()> {
    bail!("`library delete` は未実装です");
}

fn update(_store: &LocalStore, _id: Option<&str>, _path: Option<&Path>) -> anyhow::Result<()> {
    bail!("`library update` は未実装です");
}

fn list(_store: &LocalStore) -> anyhow::Result<()> {
    bail!("`library list` は未実装です");
}

fn show(_store: &LocalStore, _id: &str) -> anyhow::Result<()> {
    bail!("`library show` は未実装です");
}
