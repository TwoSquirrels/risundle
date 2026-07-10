//! 相対パスを `/` 区切りの文字列へ正規化する共有ヘルパー。
//! ダミー生成の `#include` パスと、`tags.json` の `files` キーで同じ表現を使うため一箇所にまとめる。

use std::path::{Component, Path};

use anyhow::{Context, Result};

/// 相対パスを `/` 区切りの文字列へ変換する。
///
/// `strip_prefix` 後の相対パスは通常要素のみで構成されるため、`Normal` 以外は現れない。
/// 非 UTF-8 のファイル名は `#include` パスや `tags.json` のキーたり得ないためエラーとする。
pub fn to_slash(relative: &Path) -> Result<String> {
    let mut parts = Vec::new();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            continue;
        };
        let name = name
            .to_str()
            .with_context(|| format!("file name is not valid UTF-8: {}", relative.display()))?;
        parts.push(name);
    }
    Ok(parts.join("/"))
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    #[test]
    fn rejects_non_utf8_names() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        use std::path::Path;

        // 非 UTF-8 のファイル名は #include パスにも tags.json のキーにもなれないため弾く。
        let relative = Path::new(OsStr::from_bytes(b"lib/\xff.hpp"));
        assert!(super::to_slash(relative).is_err());
    }
}
