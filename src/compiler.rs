//! コンパイラへの問い合わせ。ライブラリ登録とバンドルのどちらにも属さない共有の道具として、
//! コンパイラ指定の正規化 ([`resolve`]) とシステム include パスの検出 ([`system_includes`]) を担う。
//!
//! `resolve` は `g++` と `/usr/bin/g++` を別物と扱わないことが肝。両者を同じ絶対パスに正規化する
//! ことで、std の認識コンパイラ集合とバンドル時のコンパイラの照合が正しく一致する。
//!
//! ただしシンボリックリンクは辿らない。`canonicalize` で実体まで辿ると、`clang++` (実体は `clang`
//! への symlink) が `clang` に化けて区別できなくなり、`g++` も `x86_64-linux-gnu-g++-14` のような
//! 実体名になってしまう。表記揺れの解消に必要なのは絶対パス化だけで、symlink 解決はやり過ぎ。

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, anyhow, bail};

use crate::fs::which::{existing_executable, find_in_path};

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
        existing_executable(absolute)
            .ok_or_else(|| anyhow!("compiler {} not found", compiler.display()))?
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

/// コンパイラのシステム include パス一覧を検出する。
///
/// `-v` 付きプリプロセスの標準エラーに出る探索リストを解析する。`CPATH` 等の環境変数は探索パスを
/// 汚染する (ユーザーのライブラリが紛れる) ため取り除き、コンパイラ本来のシステム dir だけを得る。
/// `parse_search_dirs` が探す目印文字列は英語固定なので、`LC_ALL`/`LANGUAGE` を `C` に固定し、
/// 非英語ロケール環境でも gcc/clang の診断メッセージが翻訳されないようにする。
pub fn system_includes(compiler: &Path) -> Result<Vec<PathBuf>> {
    let output = Command::new(compiler)
        .args(["-E", "-x", "c++", "-v", "-"])
        .env_remove("CPATH")
        .env_remove("C_INCLUDE_PATH")
        .env_remove("CPLUS_INCLUDE_PATH")
        .env("LC_ALL", "C")
        .env("LANGUAGE", "C")
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
            // dunce 版 canonicalize で Windows の verbatim パス化を避ける (registry 側の解決と同じ規則)。
            dir.is_dir()
                .then(|| dunce::canonicalize(&dir).ok())
                .flatten()
        })
        .collect()
}

/// テスト用の偽コンパイラ生成。コンパイラ起動を伴う処理を、実コンパイラ無しで決定的にテスト
/// するための共有補助。依存を内向きに保つため終端の本モジュールが持ち、`library` 側のテスト補助
/// ([`crate::library::testutil`]) はこれを再輸出して使う。
#[cfg(test)]
pub mod testutil {
    #[cfg(unix)]
    use std::path::{Path, PathBuf};

    /// 実行可能な偽コンパイラスクリプトを作る。`-v` の出力や終了コードを自由に偽装できる。
    #[cfg(unix)]
    pub fn fake_compiler(dir: &Path, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join("fake-cc");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();

        // 書いた直後のスクリプトは、起動がまれに ETXTBSY (Text file busy) で失敗する: 並列テストの
        // 別スレッドが書き込み中に fork すると、複製された書き込み fd が子プロセスの exec まで
        // 生き残るため (Linux の既知の競合)。そこで、起動できることを空実行で確かめてから返す。
        // 一度起動できたなら書き込み fd はもう残っていないので、呼び出し側は安全に起動できる。
        // ETXTBSY 以外の失敗は原因が別 (権限や形式の問題) なので、後段の分かりにくい失敗に
        // 化けないようここで即座に落とす。
        let mut busy_attempts = 0;
        loop {
            let probe = std::process::Command::new(&path)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
            match probe {
                Ok(_) => break,
                Err(err) if err.kind() == std::io::ErrorKind::ExecutableFileBusy => {
                    busy_attempts += 1;
                    assert!(
                        busy_attempts < 100,
                        "偽コンパイラ {} が ETXTBSY のまま起動可能にならない",
                        path.display()
                    );
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(err) => panic!("偽コンパイラ {} の起動確認に失敗: {err}", path.display()),
            }
        }
        path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::fs;

    use tempfile::TempDir;

    #[cfg(unix)]
    use super::testutil::fake_compiler;

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
    fn parses_search_dirs_between_markers() {
        let verbose = "ignored preamble\n\
            #include \"...\" search starts here:\n\
            #include <...> search starts here:\n \
            /nonexistent/should/skip\n \
            .\n\
            End of search list.\n\
            trailing junk\n";
        // 実在する dir のみ realpath 化される。"." はカレントなので拾われる。
        // 期待値も関数と同じ正規化 (dunce) を通して比較する: macOS の symlink (/tmp→/private/tmp)
        // などで素のパスとは表記が分岐するため。
        let dirs = parse_search_dirs(verbose);
        assert_eq!(dirs, vec![dunce::canonicalize(Path::new(".")).unwrap()]);
    }

    #[cfg(unix)]
    #[test]
    fn system_includes_parses_the_verbose_search_list() {
        let temp = TempDir::new().unwrap();
        let include_dir = temp.path().join("include");
        fs::create_dir(&include_dir).unwrap();
        let cc = fake_compiler(
            temp.path(),
            &format!(
                "echo '#include <...> search starts here:' >&2\n\
                 echo ' {}' >&2\n\
                 echo 'End of search list.' >&2",
                include_dir.display()
            ),
        );

        assert_eq!(
            system_includes(&cc).unwrap(),
            vec![dunce::canonicalize(&include_dir).unwrap()]
        );
    }

    #[cfg(unix)]
    #[test]
    fn system_includes_forces_the_c_locale() {
        // 目印文字列 (`#include <...> search starts here:` 等) は英語固定でパースするため、
        // 非英語ロケール環境でも gcc/clang の出力自体が英語のままになるよう LC_ALL/LANGUAGE を
        // 固定している (#73)。偽コンパイラは、それらが `C` でなければ翻訳済みの目印を返すことで、
        // 呼び出し側が明示的にロケールを固定していることを確かめる。
        let temp = TempDir::new().unwrap();
        let include_dir = temp.path().join("include");
        fs::create_dir(&include_dir).unwrap();
        let cc = fake_compiler(
            temp.path(),
            &format!(
                "if [ \"$LC_ALL\" = C ] && [ \"$LANGUAGE\" = C ]; then\n\
                 \x20 echo '#include <...> search starts here:' >&2\n\
                 \x20 echo ' {}' >&2\n\
                 \x20 echo 'End of search list.' >&2\n\
                 else\n\
                 \x20 echo '#include <...> の検索はここから始まります:' >&2\n\
                 \x20 echo ' {}' >&2\n\
                 \x20 echo '検索リストの終わりです。' >&2\n\
                 fi",
                include_dir.display(),
                include_dir.display()
            ),
        );

        assert_eq!(
            system_includes(&cc).unwrap(),
            vec![dunce::canonicalize(&include_dir).unwrap()]
        );
    }

    #[cfg(unix)]
    #[test]
    fn system_includes_reports_compiler_failure_with_its_stderr() {
        let temp = TempDir::new().unwrap();
        let cc = fake_compiler(temp.path(), "echo 'unsupported option' >&2\nexit 1");

        let err = system_includes(&cc).unwrap_err().to_string();
        assert!(
            err.contains("failed to detect the system include paths"),
            "{err}"
        );
        assert!(
            err.contains("unsupported option"),
            "原因特定のためコンパイラの標準エラーを含めるべき: {err}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn system_includes_requires_a_nonempty_search_list() {
        // 正常終了しても探索リストが空なら、後段で壊れる前にここで失敗する (フェイルファスト)。
        let temp = TempDir::new().unwrap();
        let cc = fake_compiler(
            temp.path(),
            "echo '#include <...> search starts here:' >&2\necho 'End of search list.' >&2",
        );

        let err = system_includes(&cc).unwrap_err().to_string();
        assert!(
            err.contains("could not detect any system include paths"),
            "{err}"
        );
    }

    #[test]
    fn system_includes_reports_launch_failure() {
        // ディレクトリは実行できないため、プロセス起動自体が OS エラーになる経路を通す。
        let temp = TempDir::new().unwrap();
        let err = system_includes(temp.path()).unwrap_err().to_string();
        assert!(err.contains("failed to launch compiler"), "{err}");
    }
}
