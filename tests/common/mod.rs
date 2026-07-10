//! E2E テストの共通基盤。各テストを独立した `$LOCAL` (`XDG_DATA_HOME`) と作業ディレクトリへ隔離し、
//! 実バイナリを CLI 経由で起動する。内部構造に踏み込まず、ユーザーが叩くコマンドだけを通すことで、
//! 実装変更に強い E2E を保つ。

#![allow(dead_code)] // 各テストファイルがヘルパーの一部だけを使うため。

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use assert_cmd::cargo::CommandCargoExt;
use tempfile::TempDir;

/// 実 `add-std` を一度だけ行い、その結果をテンプレートとして保持する。
///
/// `add-std` は g++ のシステム include 全体を走査するため数秒かかる。テストごとに繰り返すと
/// 総時間が嵩むので、プロセス内で 1 回だけ実行し、各サンドボックスへは複製して配る。
fn std_template() -> &'static Path {
    static TEMPLATE: OnceLock<TempDir> = OnceLock::new();
    TEMPLATE
        .get_or_init(|| {
            let data = TempDir::new().expect("create std template dir");
            let status = Command::cargo_bin("risundle")
                .expect("locate risundle binary")
                .args(["library", "add-std"])
                .env("XDG_DATA_HOME", data.path())
                .status()
                .expect("run library add-std");
            assert!(
                status.success(),
                "`risundle library add-std` に失敗 (E2E には g++ が必要)"
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

/// バンドル済みソースを g++ でコンパイル・実行し、標準出力を返す。
///
/// コンパイルが通ること自体が「必要なヘッダーが過不足なく残った」ことの証明になる。
pub fn compile_and_run(sandbox: &Sandbox, bundled: &str) -> String {
    let source = sandbox.work_dir().join("bundled.cpp");
    std::fs::write(&source, bundled).expect("write bundled source");
    let binary = sandbox.work_dir().join("bundled.out");

    let compiled = Command::new("g++")
        .args(["-std=c++17", "-O0", "-o"])
        .arg(&binary)
        .arg(&source)
        .output()
        .expect("run g++");
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
