//! ライブラリのダミーファイル生成。`<source_root>` 以下のディレクトリ構造をそのまま写し取り、
//! 各ファイルを `#pragma RISUNDLE_DUMMY <相対パス>` だけの内容に置き換えて `<dummy_root>` 以下へ
//! 出力する。バンドル時、維持指定ライブラリの `-I` をこのダミーへ向け、pragma を後段で
//! `#include` へ戻すことで、当該ライブラリを Tree-Shaking 対象から除外する。

use std::path::Path;

use anyhow::{Context, Result};

use crate::fs::{relpath, source};

const DUMMY_PRAGMA: &str = "RISUNDLE_DUMMY";

/// `source_root` 以下の全ファイルについて、同じ相対パスのダミーを `dummy_root` 以下に生成する。
///
/// 既存の `dummy_root` を空にする責務は持たない (呼び出し側がディレクトリごと作り直す前提)。
pub fn generate(source_root: &Path, dummy_root: &Path) -> Result<()> {
    source::walk_sources(source_root, |relative, _content| {
        write_dummy(relative, dummy_root)
    })
}

fn write_dummy(relative: &Path, dummy_root: &Path) -> Result<()> {
    let destination = dummy_root.join(relative);
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("{} の作成に失敗しました", parent.display()))?;
    }
    // 復元する include は山括弧で固定する。ダミーが表すのは必ず `-I` で解決される維持ライブラリで
    // あり、山括弧が C++ の慣習に沿う。ユーザーが引用符 (`"..."`) で書いていても、プリプロセス後の
    // pragma 行からは元の記法を区別できず (情報が失われる)、かつ山括弧へ正規化しても `-I` 経由で
    // 同じく解決されるため実害がない。
    let include = relpath::to_slash(relative)?;
    let content = format!("#pragma {DUMMY_PRAGMA} <{include}>\n");
    std::fs::write(&destination, content)
        .with_context(|| format!("{} の書き込みに失敗しました", destination.display()))
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
