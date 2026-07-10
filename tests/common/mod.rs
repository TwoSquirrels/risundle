//! E2E テストの共通基盤。各テストを独立した `$LOCAL` (`XDG_DATA_HOME`) と作業ディレクトリへ隔離し、
//! 実バイナリを CLI 経由で起動する。内部構造に踏み込まず、ユーザーが叩くコマンドだけを通すことで、
//! 実装変更に強い E2E を保つ。
//!
//! テスト対象のコンパイラは [`test_compiler`] (`RISUNDLE_TEST_COMPILER`、既定 g++) で決まり、
//! std 登録・バンドル・検証コンパイルの全工程を同じコンパイラで一貫して通す。コンパイラや OS の
//! 掛け算は CI のマトリクスに任せ、1 プロセスは 1 コンパイラの世界に保つ。

#![allow(dead_code)] // 各テストファイルがヘルパーの一部だけを使うため。

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

use assert_cmd::cargo::CommandCargoExt;
use tempfile::TempDir;

/// テスト対象のコンパイラ。`RISUNDLE_TEST_COMPILER` で切り替え、省略時は g++。
pub fn test_compiler() -> &'static str {
    static COMPILER: OnceLock<String> = OnceLock::new();
    COMPILER.get_or_init(|| {
        std::env::var("RISUNDLE_TEST_COMPILER").unwrap_or_else(|_| "g++".to_owned())
    })
}

/// テスト対象コンパイラが `<bits/stdc++.h>` を解決できるかを、プロセス内で 1 度だけ調べる。
///
/// このヘッダーは libstdc++ (GCC 系) 専用で、libc++ (macOS の Apple clang など) には存在しない。
/// 依存するテストはこれで自己スキップする。`cfg` でなく実測にするのは、環境の実態 (macOS でも
/// GCC を入れていれば走る、など) に従うため。
pub fn supports_bits_stdcxx() -> bool {
    static SUPPORTED: OnceLock<bool> = OnceLock::new();
    *SUPPORTED.get_or_init(|| {
        let mut probe = Command::new(test_compiler())
            .args(["-x", "c++", "-E", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("launch the test compiler");
        probe
            .stdin
            .take()
            .expect("open the probe's stdin")
            .write_all(b"#include <bits/stdc++.h>\n")
            .expect("write the probe source");
        probe.wait().expect("wait for the probe").success()
    })
}

/// 実 `add-std` を一度だけ行い、その結果をテンプレートとして保持する。
///
/// `add-std` はコンパイラのシステム include 全体を走査するため数秒かかる。テストごとに繰り返すと
/// 総時間が嵩むので、プロセス内で 1 回だけ実行し、各サンドボックスへは複製して配る。
fn std_template() -> &'static Path {
    static TEMPLATE: OnceLock<TempDir> = OnceLock::new();
    TEMPLATE
        .get_or_init(|| {
            let data = TempDir::new().expect("create std template dir");
            let status = Command::cargo_bin("risundle")
                .expect("locate risundle binary")
                .args(["library", "add-std", test_compiler()])
                .env("XDG_DATA_HOME", data.path())
                .status()
                .expect("run library add-std");
            assert!(
                status.success(),
                "`risundle library add-std {}` に失敗 (E2E にはテスト対象のコンパイラが必要)",
                test_compiler()
            );
            data
        })
        .path()
}

/// `tests/fixtures/<name>` (submodule の有名ライブラリ) への絶対パス。
pub fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// 1 テスト分の隔離環境。`$LOCAL` と作業ディレクトリを独立した一時領域に持つ。
pub struct Sandbox {
    data: TempDir,
    work: TempDir,
}

impl Sandbox {
    /// std を登録済みの環境。バンドルを伴うテスト向け。
    pub fn new() -> Self {
        let sandbox = Self::bare();
        copy_dir(
            &std_template().join("risundle/libraries/std"),
            &sandbox.data.path().join("risundle/libraries/std"),
        )
        .expect("copy std template");
        sandbox
    }

    /// std 未登録の素の環境。ライブラリ管理コマンドや初回セットアップのテスト向け。
    pub fn bare() -> Self {
        Self {
            data: TempDir::new().expect("create data dir"),
            work: TempDir::new().expect("create work dir"),
        }
    }

    /// `XDG_DATA_HOME` と作業ディレクトリを束ねた risundle コマンドを返す。
    pub fn risundle(&self) -> Command {
        let mut command = Command::cargo_bin("risundle").expect("locate risundle binary");
        command
            .env("XDG_DATA_HOME", self.data.path())
            .current_dir(self.work.path());
        command
    }

    /// バンドル用の risundle コマンド。テスト対象のコンパイラを `--compiler` で明示済みなので、
    /// 呼び出し側は既定コンパイラ (g++) 前提を持ち込まずに済む。
    pub fn bundle_command(&self) -> Command {
        let mut command = self.risundle();
        command.args(["--compiler", test_compiler()]);
        command
    }

    /// 作業ディレクトリにファイルを書き、その絶対パスを返す。
    pub fn write(&self, relative: &str, content: &str) -> PathBuf {
        let path = self.work.path().join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create work subdir");
        }
        std::fs::write(&path, content).expect("write work file");
        path
    }

    pub fn work_dir(&self) -> &Path {
        self.work.path()
    }
}

impl Default for Sandbox {
    fn default() -> Self {
        Self::new()
    }
}

/// バンドル済みソースをテスト対象のコンパイラでコンパイル・実行し、標準出力を返す。
///
/// コンパイルが通ること自体が「必要なヘッダーが過不足なく残った」ことの証明になる。
pub fn compile_and_run(sandbox: &Sandbox, bundled: &str) -> String {
    let source = sandbox.work_dir().join("bundled.cpp");
    std::fs::write(&source, bundled).expect("write bundled source");
    let binary = sandbox.work_dir().join("bundled.out");

    let compiled = Command::new(test_compiler())
        .args(["-std=c++17", "-O0", "-o"])
        .arg(&binary)
        .arg(&source)
        .output()
        .expect("run the test compiler");
    assert!(
        compiled.status.success(),
        "バンドル結果のコンパイルに失敗:\n{}",
        String::from_utf8_lossy(&compiled.stderr)
    );

    let run = Command::new(&binary).output().expect("run compiled binary");
    assert!(run.status.success(), "コンパイル済みバイナリの実行に失敗");
    String::from_utf8(run.stdout).expect("stdout is utf-8")
}

fn copy_dir(source: &Path, destination: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(destination)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let target = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}
