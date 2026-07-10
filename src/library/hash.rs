//! ライブラリ内容の集約ハッシュ計算。`<path>` 以下の全ファイルの相対パスと内容から sha256 を
//! 計算し、ライブラリの更新検知に使う。mtime でなく内容ベースのため `git clone`/`cp` での時刻変化に
//! 左右されず、相対パスも含めるためファイルの追加・削除・リネームも検知できる。

use std::fmt::Write as _;
use std::path::Path;

use anyhow::Result;
use sha2::{Digest, Sha256};

use crate::fs::{relpath, source};

/// `root` 以下の全ファイルから集約ハッシュを計算し、`sha256:` プレフィックス付きで返す。
pub fn aggregate(root: &Path) -> Result<String> {
    let mut entries = Vec::new();
    source::walk_sources(root, |relative, content| {
        entries.push((relpath::to_slash(relative)?, content.to_vec()));
        Ok(())
    })?;
    // OS 依存の列挙順に左右されないよう、相対パスで整列して決定的にする。
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    let mut hasher = Sha256::new();
    for (relative, content) in entries {
        // パスと内容の境界を長さで明示し、異なるファイル構成が同じバイト列へ化けるのを防ぐ。
        hasher.update((relative.len() as u64).to_le_bytes());
        hasher.update(relative.as_bytes());
        hasher.update((content.len() as u64).to_le_bytes());
        hasher.update(&content);
    }

    // sha256 のダイジェストは 32 バイト = 16 進 64 文字で固定。
    let hex = hasher
        .finalize()
        .iter()
        .fold(String::with_capacity(64), |mut hex, b| {
            let _ = write!(hex, "{b:02x}");
            hex
        });
    Ok(format!("sha256:{hex}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::fs;

    use tempfile::TempDir;

    fn write_file(root: &Path, relative: &str, content: &str) {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    fn library(files: &[(&str, &str)]) -> (TempDir, String) {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("lib");
        for (rel, content) in files {
            write_file(&root, rel, content);
        }
        let hash = aggregate(&root).unwrap();
        (temp, hash)
    }

    #[test]
    fn has_sha256_prefix() {
        let (_t, hash) = library(&[("a.hpp", "int a;")]);
        assert!(hash.starts_with("sha256:"));
        // sha256: + 64 桁の 16 進数。
        assert_eq!(hash.len(), "sha256:".len() + 64);
    }

    #[test]
    fn same_content_yields_same_hash() {
        let (_t1, h1) = library(&[("a.hpp", "x"), ("dir/b.hpp", "y")]);
        let (_t2, h2) = library(&[("a.hpp", "x"), ("dir/b.hpp", "y")]);
        assert_eq!(h1, h2);
    }

    #[test]
    fn content_change_changes_hash() {
        let (_t1, h1) = library(&[("a.hpp", "x")]);
        let (_t2, h2) = library(&[("a.hpp", "y")]);
        assert_ne!(h1, h2);
    }

    #[test]
    fn adding_a_file_changes_hash() {
        let (_t1, h1) = library(&[("a.hpp", "x")]);
        let (_t2, h2) = library(&[("a.hpp", "x"), ("b.hpp", "")]);
        assert_ne!(h1, h2);
    }

    #[test]
    fn renaming_a_file_changes_hash() {
        // 内容が同じでも相対パスが変わればハッシュも変わる (リネーム検知)。
        let (_t1, h1) = library(&[("a.hpp", "x")]);
        let (_t2, h2) = library(&[("b.hpp", "x")]);
        assert_ne!(h1, h2);
    }

    #[test]
    fn path_content_boundary_is_unambiguous() {
        // 長さ明示が無いと衝突しうる構成。(path="a", content="bc") と (path="ab", content="c")。
        let (_t1, h1) = library(&[("a", "bc")]);
        let (_t2, h2) = library(&[("ab", "c")]);
        assert_ne!(h1, h2);
    }

    #[test]
    fn empty_library_hashes_the_empty_input() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("empty");
        fs::create_dir_all(&root).unwrap();
        // ファイルが一つも無ければ、空入力の sha256 になる。
        assert_eq!(
            aggregate(&root).unwrap(),
            "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn errors_when_root_is_missing() {
        let temp = TempDir::new().unwrap();
        assert!(aggregate(&temp.path().join("nonexistent")).is_err());
    }
}
