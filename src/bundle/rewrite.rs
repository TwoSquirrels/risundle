//! プリプロセス出力の置換。上から 1 行ずつ走査し、(1) 不要ヘッダー由来のコード行を削除し、
//! (2) ダミーの `#pragma RISUNDLE_DUMMY <...>` を `#include <...>` へ復元する。linemarker は
//! そのまま残す (バンドル後のコンパイラ診断の行番号が元ファイルを指すようにするため)。
//!
//! 行の出所は [`Tracker`] が追う。これにより、不要ヘッダーのスコープ内にネストされた別ヘッダーの
//! 行は、そのヘッダー自身の要否で独立に判断される (linemarker が出所を切り替えるたびに再評価)。

use crate::bundle::linemarker::{Line, Tracker};
use crate::library::dummy::DUMMY_PRAGMA;

/// プリプロセス結果を走査し、置換済みのコードを返す。
///
/// `is_unused` は出所ファイル (linemarker のパス文字列) が不要ヘッダーかを判定する。不要と判定された
/// 出所のコード行だけを削除し、linemarker と他ファイルの行は残す。
pub fn rewrite(preprocessed: &str, is_unused: impl Fn(&str) -> bool) -> String {
    let mut tracker = Tracker::new();
    let mut output = String::new();
    for line in preprocessed.lines() {
        match tracker.observe(line) {
            // linemarker は出力にそのまま残す。
            Line::Marker(_) => push_line(&mut output, line),
            Line::Code { file } => {
                if file.is_some_and(&is_unused) {
                    continue; // 不要ヘッダー由来のコード行は削除
                }
                match restore_include(line) {
                    Some(include) => push_line(&mut output, &include),
                    None => push_line(&mut output, line),
                }
            }
        }
    }
    output
}

fn push_line(output: &mut String, line: &str) {
    output.push_str(line);
    output.push('\n');
}

/// ダミーの pragma 行なら `#include <...>` を返す。それ以外は `None`。
///
/// 復元する include は山括弧形式に固定する。ダミーが表すのは必ず `-I` で解決される維持ライブラリで
/// あり、pragma 行からは元が `<>` か `""` か区別できないため。引用符で書かれていても山括弧へ正規化
/// する。
fn restore_include(line: &str) -> Option<String> {
    let target = line
        .trim_start()
        .strip_prefix("#pragma")?
        .trim_start()
        .strip_prefix(DUMMY_PRAGMA)?
        .trim();
    let inner = strip_brackets(target)?;
    Some(format!("#include <{inner}>"))
}

/// `<path>` または `"path"` の中身を取り出す。
fn strip_brackets(token: &str) -> Option<&str> {
    token
        .strip_prefix('<')
        .and_then(|rest| rest.strip_suffix('>'))
        .or_else(|| {
            token
                .strip_prefix('"')
                .and_then(|rest| rest.strip_suffix('"'))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::BTreeSet;

    fn unused(files: &[&str]) -> impl Fn(&str) -> bool {
        let set: BTreeSet<String> = files.iter().map(|f| (*f).to_owned()).collect();
        move |file| set.contains(file)
    }

    #[test]
    fn keeps_linemarkers_and_target_code() {
        let input = "# 1 \"main.cpp\"\nint main() {}\n";
        assert_eq!(rewrite(input, unused(&[])), input);
    }

    #[test]
    fn deletes_code_lines_of_unused_headers_but_keeps_markers() {
        let input = "# 1 \"main.cpp\"\nint main() {}\n# 1 \"/lib/dead.hpp\"\nstruct Dead {};\n# 2 \"main.cpp\"\nint x;\n";
        let output = rewrite(input, unused(&["/lib/dead.hpp"]));

        assert!(output.contains("int main() {}"));
        assert!(output.contains("int x;"));
        assert!(!output.contains("struct Dead {};")); // 不要ヘッダーのコードは削除
        assert!(output.contains("# 1 \"/lib/dead.hpp\"")); // linemarker は残す
    }

    #[test]
    fn nested_header_is_judged_independently() {
        // 不要ヘッダー A の途中で必要ヘッダー B に切り替わると、B の行は残る。
        let input = "# 1 \"/lib/A.hpp\"\nstruct A {};\n# 1 \"/lib/B.hpp\"\nstruct B {};\n# 5 \"/lib/A.hpp\"\nstruct A2 {};\n";
        let output = rewrite(input, unused(&["/lib/A.hpp"]));

        assert!(!output.contains("struct A {};"));
        assert!(!output.contains("struct A2 {};"));
        assert!(output.contains("struct B {};")); // B は不要指定されていないので残る
    }

    #[test]
    fn restores_dummy_pragma_to_angle_include() {
        let input = "# 1 \"/local/dummy/atcoder/dsu\"\n#pragma RISUNDLE_DUMMY <atcoder/dsu>\n";
        let output = rewrite(input, unused(&[]));
        assert!(output.contains("#include <atcoder/dsu>"));
        assert!(!output.contains("#pragma RISUNDLE_DUMMY"));
    }

    #[test]
    fn normalizes_quoted_dummy_to_angle_include() {
        let output = rewrite("#pragma RISUNDLE_DUMMY \"atcoder/dsu\"\n", unused(&[]));
        assert!(output.contains("#include <atcoder/dsu>"));
    }

    #[test]
    fn leaves_unrelated_pragmas_untouched() {
        let input = "#pragma once\n#pragma GCC optimize(\"O2\")\n";
        assert_eq!(rewrite(input, unused(&[])), input);
    }
}
