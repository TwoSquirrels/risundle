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
        let name = name.to_str().with_context(|| {
            format!("ファイル名が UTF-8 ではありません: {}", relative.display())
        })?;
        parts.push(name);
    }
    Ok(parts.join("/"))
}
