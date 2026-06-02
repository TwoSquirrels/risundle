//! ライブラリ内の「ソースファイル」だけを選別して走査する共通レイヤ。バンドル対象になりうるのは
//! C++ のヘッダ・ソースのみのため、それ以外の拡張子 (`.md` / `.txt` / `.py` 等) を除外する。
//! 拡張子を持たないファイル (AC Library の `atcoder/modint` のような include 専用ファイル) は
//! C++ か否か判別できないため通すが、内容に NUL バイトを含むバイナリは拡張子の有無に関わらず弾く。
//! ドット始まりの隠しファイル・ディレクトリ (`.git/` `.clang-format` 等) は VCS・設定メタデータで
//! あって include 対象になりえないため、パスのどこかに含まれた時点で除外する。特に `.git/` を含めると
//! commit のたびに集約ハッシュが変わり、ライブラリ更新を誤検知してしまう。
//!
//! ダミー生成・識別子列挙・ハッシュ計算がいずれもこの同じ選別を共有することで、3 者が常に同一の
//! ファイル集合を対象にする。対象がずれると `tags.json` の `files` と更新検知用ハッシュが食い違う。

// add / update コマンドが消費するまでは未使用のため、実装が揃うまで明示的に許可する。
#![allow(dead_code)]

use std::path::{Component, Path};

use anyhow::{Context, Result};

use crate::walk;

/// ソースとして扱う拡張子。競技プログラミングのライブラリに現れる C++ のヘッダ・ソースを網羅する。
const SOURCE_EXTENSIONS: &[&str] = &[
    "h", "hpp", "hh", "hxx", "h++", "ipp", "tcc", "inc", "c", "cpp", "cc", "cxx", "c++",
];

/// `root` 以下のソースファイルについて `visit(相対パス, 内容)` を呼ぶ。
///
/// 選別は隠し要素 (`is_hidden`)・拡張子 (`has_source_extension`)・内容 (`looks_like_text`) で行う。
/// 隠し要素と非ソースは読み取りすらせずスキップし、バイナリは読み取り後に弾く。シンボリックリンクは
/// 辿らない (`walk::walk_files` に従う)。
pub fn walk_sources(root: &Path, mut visit: impl FnMut(&Path, &[u8]) -> Result<()>) -> Result<()> {
    walk::walk_files(root, |relative, absolute| {
        if is_hidden(relative) || !has_source_extension(relative) {
            return Ok(());
        }
        let content = std::fs::read(absolute)
            .with_context(|| format!("{} の読み取りに失敗しました", absolute.display()))?;
        if !looks_like_text(&content) {
            return Ok(());
        }
        visit(relative, &content)
    })
}

/// パスのいずれかの要素がドット始まり (隠しファイル・ディレクトリ) なら真。
fn is_hidden(relative: &Path) -> bool {
    relative.components().any(|component| {
        matches!(component, Component::Normal(name)
            if name.to_str().is_some_and(|name| name.starts_with('.')))
    })
}

/// ソース拡張子を持つか、または拡張子を持たない場合に真。拡張子は大文字小文字を区別しない (`.H` 等)。
fn has_source_extension(relative: &Path) -> bool {
    match relative.extension() {
        None => true,
        Some(extension) => extension.to_str().is_some_and(|extension| {
            SOURCE_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str())
        }),
    }
}

/// NUL バイトを含まなければテキストとみなす。バイナリファイルを弾くための簡易判定。
fn looks_like_text(content: &[u8]) -> bool {
    !content.contains(&0)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::BTreeSet;
    use std::fs;
    use std::path::PathBuf;

    use tempfile::TempDir;

    fn write_file(root: &Path, relative: &str, content: &[u8]) {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    fn visited_paths(root: &Path) -> BTreeSet<PathBuf> {
        let mut seen = BTreeSet::new();
        walk_sources(root, |relative, _content| {
            seen.insert(relative.to_path_buf());
            Ok(())
        })
        .unwrap();
        seen
    }

    #[test]
    fn passes_source_extensions() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("lib");
        for name in ["a.hpp", "b.h", "c.cpp", "d.cc", "e.hxx", "f.tcc"] {
            write_file(&root, name, b"int x;");
        }

        let expected: BTreeSet<PathBuf> = ["a.hpp", "b.h", "c.cpp", "d.cc", "e.hxx", "f.tcc"]
            .iter()
            .map(PathBuf::from)
            .collect();
        assert_eq!(visited_paths(&root), expected);
    }

    #[test]
    fn passes_extensionless_files() {
        // AC Library の `atcoder/modint` のように、include 専用の拡張子なしファイルは通す。
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("lib");
        write_file(&root, "atcoder/modint", b"#include <atcoder/modint.hpp>");

        assert!(visited_paths(&root).contains(&PathBuf::from("atcoder/modint")));
    }

    #[test]
    fn rejects_non_source_extensions() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("lib");
        write_file(&root, "README.md", b"# title");
        write_file(&root, "build.py", b"print(1)");
        write_file(&root, "data.json", b"{}");
        write_file(&root, "keep.hpp", b"int x;");

        assert_eq!(
            visited_paths(&root),
            BTreeSet::from([PathBuf::from("keep.hpp")])
        );
    }

    #[test]
    fn rejects_binary_even_with_source_extension() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("lib");
        write_file(&root, "blob.h", b"\x00\x01\x02binary");
        write_file(&root, "text.h", b"int x;");

        assert_eq!(
            visited_paths(&root),
            BTreeSet::from([PathBuf::from("text.h")])
        );
    }

    #[test]
    fn extension_match_is_case_insensitive() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("lib");
        write_file(&root, "a.HPP", b"int x;");
        write_file(&root, "b.H", b"int y;");

        let expected = BTreeSet::from([PathBuf::from("a.HPP"), PathBuf::from("b.H")]);
        assert_eq!(visited_paths(&root), expected);
    }

    #[test]
    fn rejects_hidden_files_and_directories() {
        // .git/ 配下や .clang-format などの隠し要素は VCS・設定メタデータであり対象外。
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("lib");
        write_file(&root, ".git/config", b"[core]");
        write_file(&root, ".gitignore", b"target");
        write_file(&root, ".clang-format", b"BasedOnStyle: Google");
        write_file(&root, "atcoder/dsu.hpp", b"struct dsu {};");

        assert_eq!(
            visited_paths(&root),
            BTreeSet::from([PathBuf::from("atcoder/dsu.hpp")])
        );
    }

    #[test]
    fn passes_content_to_visitor() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("lib");
        write_file(&root, "a.hpp", b"struct modint {};");

        let mut content = Vec::new();
        walk_sources(&root, |_relative, bytes| {
            content = bytes.to_vec();
            Ok(())
        })
        .unwrap();
        assert_eq!(content, b"struct modint {};");
    }
}
