//! コンパイラ指定の正規化。`add-std` (std 登録) とバンドルが、同じコンパイラを表記揺れなく突き
//! 合わせられるよう、与えられたコンパイラ名/パスを絶対パスへ解決する。
//!
//! `g++` と `/usr/bin/g++` を別物と扱わないことが肝。両者を同じ絶対パスに正規化することで、std の
//! 認識コンパイラ集合とバンドル時のコンパイラの照合が正しく一致する。
//!
//! ただしシンボリックリンクは辿らない。`canonicalize` で実体まで辿ると、`clang++` (実体は `clang`
//! への symlink) が `clang` に化けて区別できなくなり、`g++` も `x86_64-linux-gnu-g++-14` のような
//! 実体名になってしまう。表記揺れの解消に必要なのは絶対パス化だけで、symlink 解決はやり過ぎ。

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};

/// コンパイラ名/パスを絶対パスへ解決する (シンボリックリンクは辿らない)。
///
/// パス区切りを含む指定は絶対パス化して存在を確認し、`g++` のような素の名前は `PATH` を探索して
/// 実体のあるパスを得る。`clang++` と `clang` のように symlink で実体を共有するコンパイラも、
/// 指定された名前のまま区別される。
pub fn resolve(compiler: &Path) -> Result<PathBuf> {
    let located = if compiler.components().count() > 1 {
        let absolute = std::path::absolute(compiler).with_context(|| {
            format!(
                "failed to make an absolute path for compiler {}",
                compiler.display()
            )
        })?;
        if !absolute.is_file() {
            bail!("compiler {} not found", compiler.display());
        }
        absolute
    } else {
        let path_var = std::env::var_os("PATH").unwrap_or_default();
        find_in_path(compiler, &path_var)
            .ok_or_else(|| anyhow!("compiler {} not found in PATH", compiler.display()))?
    };
    // PATH のエントリが相対 (例: `.`) のこともあるため、最終的に絶対パスへ揃える。
    std::path::absolute(&located).with_context(|| {
        format!(
            "failed to make an absolute path for compiler {}",
            compiler.display()
        )
    })
}

/// `PATH` の各ディレクトリから `name` の実行ファイルを探し、最初に見つかったパスを返す。
fn find_in_path(name: &Path, path_var: &OsStr) -> Option<PathBuf> {
    std::env::split_paths(path_var)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::fs;

    use tempfile::TempDir;

    #[test]
    fn resolves_path_with_separator_to_absolute() {
        let temp = TempDir::new().unwrap();
        let bin = temp.path().join("g++");
        fs::write(&bin, "").unwrap();

        assert_eq!(resolve(&bin).unwrap(), std::path::absolute(&bin).unwrap());
    }

    #[test]
    fn missing_path_with_separator_errors() {
        let temp = TempDir::new().unwrap();
        assert!(resolve(&temp.path().join("nonexistent-cc")).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn does_not_follow_symlinks() {
        // clang++ が clang への symlink でも、実体 clang に化けず clang++ のまま区別される。
        let temp = TempDir::new().unwrap();
        let real = temp.path().join("clang");
        fs::write(&real, "").unwrap();
        let link = temp.path().join("clang++");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        let resolved = resolve(&link).unwrap();
        assert_eq!(resolved, std::path::absolute(&link).unwrap());
        assert_ne!(resolved, resolve(&real).unwrap());
    }

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
}
