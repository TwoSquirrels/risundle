//! ライブラリのダミーファイル生成。`<source_root>` 以下のディレクトリ構造をそのまま写し取り、
//! 各ファイルを `#pragma RISUNDLE_DUMMY <相対パス>` だけの内容に置き換えて `<dummy_root>` 以下へ
//! 出力する。バンドル時、維持指定ライブラリの `-I` をこのダミーへ向け、pragma を後段で
//! `#include` へ戻すことで、当該ライブラリを Tree-Shaking 対象から除外する。

// add / update コマンドが消費するまでは未使用のため、実装が揃うまで明示的に許可する。
#![allow(dead_code)]

use std::path::{Component, Path};

use anyhow::{Context, Result};

const DUMMY_PRAGMA: &str = "RISUNDLE_DUMMY";

/// `source_root` 以下の全ファイルについて、同じ相対パスのダミーを `dummy_root` 以下に生成する。
///
/// 既存の `dummy_root` を空にする責務は持たない (呼び出し側がディレクトリごと作り直す前提)。
/// シンボリックリンクは辿らない (循環を避けるため v1.0 では対象外)。
pub fn generate(source_root: &Path, dummy_root: &Path) -> Result<()> {
    generate_dir(source_root, source_root, dummy_root)
}

fn generate_dir(source_root: &Path, current: &Path, dummy_root: &Path) -> Result<()> {
    let entries = std::fs::read_dir(current)
        .with_context(|| format!("{} の読み取りに失敗しました", current.display()))?;
    for entry in entries {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let path = entry.path();
        if file_type.is_dir() {
            generate_dir(source_root, &path, dummy_root)?;
        } else if file_type.is_file() {
            let relative = path
                .strip_prefix(source_root)
                .expect("read_dir のエントリは source_root 配下にある");
            write_dummy(relative, dummy_root)?;
        }
    }
    Ok(())
}

fn write_dummy(relative: &Path, dummy_root: &Path) -> Result<()> {
    let destination = dummy_root.join(relative);
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("{} の作成に失敗しました", parent.display()))?;
    }
    let include = to_include_path(relative)?;
    let content = format!("#pragma {DUMMY_PRAGMA} <{include}>\n");
    std::fs::write(&destination, content)
        .with_context(|| format!("{} の書き込みに失敗しました", destination.display()))
}

/// 相対パスを `#include` 用の `/` 区切り文字列へ変換する。
///
/// `strip_prefix` 後の相対パスは通常要素のみで構成されるため、`Normal` 以外は現れない。
/// 非 UTF-8 のファイル名は `#include` パスたり得ないためエラーとする。
fn to_include_path(relative: &Path) -> Result<String> {
    let mut parts = Vec::new();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            continue;
        };
        let name = name.to_str().with_context(|| {
            format!("ファイル名が UTF-8 ではありません: {}", relative.display())
        })?;
        parts.push(name);
    }
    Ok(parts.join("/"))
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

    #[test]
    fn mirrors_directory_structure_into_dummy_root() {
        let temp = TempDir::new().unwrap();
        let source = temp.path().join("source");
        let dummy = temp.path().join("dummy");

        write_file(&source, "atcoder/modint.hpp", "struct modint {};");
        write_file(&source, "atcoder/segtree.hpp", "struct segtree {};");

        generate(&source, &dummy).unwrap();

        assert!(dummy.join("atcoder/modint.hpp").is_file());
        assert!(dummy.join("atcoder/segtree.hpp").is_file());
    }

    #[test]
    fn dummy_content_is_pragma_with_relative_path() {
        let temp = TempDir::new().unwrap();
        let source = temp.path().join("source");
        let dummy = temp.path().join("dummy");

        write_file(&source, "atcoder/modint.hpp", "struct modint {};");

        generate(&source, &dummy).unwrap();

        let content = fs::read_to_string(dummy.join("atcoder/modint.hpp")).unwrap();
        assert_eq!(content, "#pragma RISUNDLE_DUMMY <atcoder/modint.hpp>\n");
    }

    #[test]
    fn extensionless_file_keeps_its_path_verbatim() {
        // AC Library の `atcoder/modint` のように、拡張子なしのファイルもそのまま写し取る。
        let temp = TempDir::new().unwrap();
        let source = temp.path().join("source");
        let dummy = temp.path().join("dummy");

        write_file(&source, "atcoder/modint", "#include <atcoder/modint.hpp>");

        generate(&source, &dummy).unwrap();

        let content = fs::read_to_string(dummy.join("atcoder/modint")).unwrap();
        assert_eq!(content, "#pragma RISUNDLE_DUMMY <atcoder/modint>\n");
    }

    #[test]
    fn errors_when_source_root_is_missing() {
        let temp = TempDir::new().unwrap();
        let result = generate(&temp.path().join("nonexistent"), &temp.path().join("dummy"));
        assert!(result.is_err());
    }
}
