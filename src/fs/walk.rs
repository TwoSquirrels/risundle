//! ライブラリディレクトリ配下の全ファイルを走査する共通ヘルパー。ダミー生成・識別子列挙・
//! ハッシュ計算がいずれも「`root` 以下を再帰し、各ファイルを相対パス付きで処理する」点で一致するため、
//! 走査ロジック (再帰・種別判定・シンボリックリンクの扱い) を一箇所に集約する。

use std::path::Path;

use anyhow::{Context, Result};

/// `root` 以下の全ファイルについて、`visit(相対パス, 絶対パス)` を呼ぶ。
///
/// 相対パスは `root` を起点とする。ディレクトリ自体は `visit` に渡さず再帰のみ行う。
/// シンボリックリンクは辿らない (`file_type` はリンクを解決しないため、リンクは file でも dir でもなく
/// 素通りする)。循環を避けるため v1.0 ではこの挙動を意図的に採る。
pub fn walk_files(root: &Path, mut visit: impl FnMut(&Path, &Path) -> Result<()>) -> Result<()> {
    walk_dir(root, root, &mut visit)
}

fn walk_dir<F>(root: &Path, current: &Path, visit: &mut F) -> Result<()>
where
    F: FnMut(&Path, &Path) -> Result<()>,
{
    let entries = std::fs::read_dir(current)
        .with_context(|| format!("{} の読み取りに失敗しました", current.display()))?;
    for entry in entries {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let path = entry.path();
        if file_type.is_dir() {
            walk_dir(root, &path, visit)?;
        } else if file_type.is_file() {
            let relative = path
                .strip_prefix(root)
                .expect("read_dir のエントリは root 配下にある");
            visit(relative, &path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::BTreeSet;
    use std::fs;
    use std::path::PathBuf;

    use anyhow::anyhow;
    use tempfile::TempDir;

    fn write_file(root: &Path, relative: &str, content: &str) {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    #[test]
    fn visits_every_file_with_relative_and_absolute_paths() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("lib");
        write_file(&root, "a.hpp", "");
        write_file(&root, "dir/b.hpp", "");
        write_file(&root, "dir/nested/c.hpp", "");

        let mut seen = BTreeSet::new();
        walk_files(&root, |relative, absolute| {
            assert_eq!(absolute, &root.join(relative));
            seen.insert(relative.to_path_buf());
            Ok(())
        })
        .unwrap();

        let expected: BTreeSet<PathBuf> = ["a.hpp", "dir/b.hpp", "dir/nested/c.hpp"]
            .iter()
            .map(PathBuf::from)
            .collect();
        assert_eq!(seen, expected);
    }

    #[test]
    fn propagates_visitor_errors() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("lib");
        write_file(&root, "a.hpp", "");

        let result = walk_files(&root, |_, _| Err(anyhow!("visitor が失敗")));
        assert!(result.unwrap_err().to_string().contains("visitor が失敗"));
    }

    #[test]
    fn errors_when_root_is_missing() {
        let temp = TempDir::new().unwrap();
        let result = walk_files(&temp.path().join("nonexistent"), |_, _| Ok(()));
        assert!(result.is_err());
    }
}
