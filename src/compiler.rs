//! コンパイラへの問い合わせ。ライブラリ登録とバンドルのどちらにも属さない共有の道具として、
//! コンパイラ指定の正規化 ([`resolve`]) とシステム include パスの検出 ([`system_includes`]) を担う。
//!
//! `resolve` は `g++` と `/usr/bin/g++` を別物と扱わないことが肝。両者を同じ絶対パスに正規化する
//! ことで、std の認識コンパイラ集合とバンドル時のコンパイラの照合が正しく一致する。
//!
//! ただしシンボリックリンクは辿らない。`canonicalize` で実体まで辿ると、`clang++` (実体は `clang`
//! への symlink) が `clang` に化けて区別できなくなり、`g++` も `x86_64-linux-gnu-g++-14` のような
//! 実体名になってしまう。表記揺れの解消に必要なのは絶対パス化だけで、symlink 解決はやり過ぎ。

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

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

/// コンパイラのシステム include パス一覧を検出する。
///
/// `-v` 付きプリプロセスの標準エラーに出る探索リストを解析する。`CPATH` 等の環境変数は探索パスを
/// 汚染する (ユーザーのライブラリが紛れる) ため取り除き、コンパイラ本来のシステム dir だけを得る。
pub fn system_includes(compiler: &Path) -> Result<Vec<PathBuf>> {
    let output = Command::new(compiler)
        .args(["-E", "-x", "c++", "-v", "-"])
        .env_remove("CPATH")
        .env_remove("C_INCLUDE_PATH")
        .env_remove("CPLUS_INCLUDE_PATH")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .with_context(|| format!("failed to launch compiler {}", compiler.display()))?;
    if !output.status.success() {
        bail!(
            "failed to detect the system include paths of compiler {}:\n{}",
            compiler.display(),
            String::from_utf8_lossy(&output.stderr).trim_end()
        );
    }
    let roots = parse_search_dirs(&String::from_utf8_lossy(&output.stderr));
    if roots.is_empty() {
        bail!(
            "could not detect any system include paths for compiler {}",
            compiler.display()
        );
    }
    Ok(roots)
}

/// `-v` 出力から `#include <...> search starts here:` 〜 `End of search list.` の dir 一覧を取り出す。
/// 実在するディレクトリのみを realpath 化して返す。
fn parse_search_dirs(verbose_output: &str) -> Vec<PathBuf> {
    let mut lines = verbose_output.lines();
    lines
        .by_ref()
        .find(|line| line.contains("#include <...> search starts here:"));
    lines
        .take_while(|line| !line.contains("End of search list."))
        .filter_map(|line| {
            let dir = PathBuf::from(line.trim());
            dir.is_dir().then(|| dir.canonicalize().ok()).flatten()
        })
        .collect()
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

    #[test]
    fn parses_search_dirs_between_markers() {
        let verbose = "ignored preamble\n\
            #include \"...\" search starts here:\n\
            #include <...> search starts here:\n \
            /nonexistent/should/skip\n \
            .\n\
            End of search list.\n\
            trailing junk\n";
        // 実在する dir のみ realpath 化される。"." はカレントなので拾われる。
        // 期待値も同じく canonicalize する: Windows の verbatim パス (`\\?\`) や macOS の
        // symlink (/tmp→/private/tmp) で表記が分岐するため、関数と同じ正規化を通して比較する。
        let dirs = parse_search_dirs(verbose);
        assert_eq!(dirs, vec![Path::new(".").canonicalize().unwrap()]);
    }
}
