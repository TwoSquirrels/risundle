//! `PATH` 上の実行可能ファイル探索。コンパイラの解決 (`compiler::resolve`) と、更新チェックでの
//! `cargo-install-update` の有無判定など、複数のドメインが共有する。

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// `PATH` の各ディレクトリから `name` の実行ファイルを探し、最初に見つかったパスを返す。
pub fn find_in_path(name: &Path, path_var: &OsStr) -> Option<PathBuf> {
    std::env::split_paths(path_var).find_map(|dir| existing_executable(dir.join(name)))
}

/// `candidate` に実在する実行ファイルを返す。Windows では実体が `name.exe` のように拡張子付きな
/// ので、拡張子なしの指定に `.exe` を補った形も探す (`Command` がプロセス起動時に行う補完と同じ
/// 規則に揃える)。
pub fn existing_executable(candidate: PathBuf) -> Option<PathBuf> {
    if candidate.is_file() {
        return Some(candidate);
    }
    #[cfg(windows)]
    if candidate.extension().is_none() {
        let with_exe = candidate.with_extension("exe");
        if with_exe.is_file() {
            return Some(with_exe);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::fs;

    use tempfile::TempDir;

    #[test]
    fn finds_bare_name_in_path() {
        let temp = TempDir::new().unwrap();
        let bin = temp.path().join("mycc");
        fs::write(&bin, "").unwrap();

        let found = find_in_path(Path::new("mycc"), temp.path().as_os_str());
        assert_eq!(found, Some(bin));
    }

    #[test]
    fn missing_bare_name_yields_none() {
        let temp = TempDir::new().unwrap();
        assert_eq!(
            find_in_path(Path::new("nonexistent-cc"), temp.path().as_os_str()),
            None
        );
    }

    #[cfg(windows)]
    #[test]
    fn finds_bare_name_with_exe_extension_on_windows() {
        // PATH 上の実体は `mycc.exe` のように拡張子付き。拡張子なしの指定でも見つかるべき。
        let temp = TempDir::new().unwrap();
        fs::write(temp.path().join("mycc.exe"), "").unwrap();

        let found = find_in_path(Path::new("mycc"), temp.path().as_os_str());
        assert_eq!(found, Some(temp.path().join("mycc.exe")));
    }
}
