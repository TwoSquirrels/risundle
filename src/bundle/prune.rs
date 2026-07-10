//! 不要ヘッダーの特定。依存ヘッダー (= `<file>` が識別子経由で必要とするライブラリヘッダー) に
//! `gcc -M` を実行して得た「必要集合」と、出力に現れた維持指定外ライブラリのヘッダー (= 候補) を
//! 突き合わせ、必要集合に一度も現れない候補を不要ヘッダーとして返す。
//!
//! 本モジュールは純粋ロジックに徹する。`-M` の実行や realpath 正規化といった IO は外側
//! ([`crate::commands::bundle`]) が担い、ここには正規化済みのパスと `-M` の出力テキストだけが渡る。
//! これによりコンパイラ無しでユニットテストできる。

use std::collections::BTreeSet;
use std::path::PathBuf;

/// `gcc -M` の出力 (make の依存ルール) から、前提条件のパス集合を取り出す。
///
/// 出力は `target.o: prereq1 prereq2 \<改行> prereq3` の形式で、複数入力なら複数ルールが並ぶ。
/// 行継続を畳み、各ルールの `:` 以降を前提条件として集める。ターゲット (`.o`) は前提条件ではない
/// ため除外する。パス中の空白は make の流儀でバックスラッシュエスケープされるため復元する。
pub fn parse_prerequisites(make_output: &str) -> BTreeSet<PathBuf> {
    // 行継続 (バックスラッシュ + 改行) を空白へ畳み、各ルールを 1 行に収める。
    let joined = make_output.replace("\\\r\n", " ").replace("\\\n", " ");
    let mut prerequisites = BTreeSet::new();
    for line in joined.lines() {
        // ':' の左はターゲット。前提条件は右側のみ。':' を含まない行 (空行等) は無視する。
        let Some((_target, rest)) = line.split_once(':') else {
            continue;
        };
        for token in tokenize(rest) {
            prerequisites.insert(PathBuf::from(token));
        }
    }
    prerequisites
}

/// 空白区切りでトークン化する。make の流儀でエスケープされる空白 (`\ `) と `#` (`\#`) だけを
/// 復元し、それ以外のバックスラッシュはそのまま残す。無差別に「直後の 1 文字」をエスケープ扱い
/// すると、MinGW g++ が -M 出力へそのまま書く Windows パス (`C:\Users\...`) の区切りが食われて
/// パスが壊れ、必要ヘッダーを全て取りこぼす (= 全削除) 事故になる。行継続は呼び出し前に畳まれて
/// いる前提。
fn tokenize(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => match chars.peek() {
                Some(&next @ (' ' | '#')) => {
                    current.push(next);
                    chars.next();
                }
                _ => current.push('\\'),
            },
            ch if ch.is_whitespace() => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            ch => current.push(ch),
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

/// 候補ヘッダーのうち、必要集合に含まれないものを不要ヘッダーとして返す。
///
/// 双方とも呼び出し側で realpath 正規化済みであることを前提とする (同一ファイルが別表記で渡ると
/// 取りこぼし得るため)。取りこぼし = 必要なヘッダーを不要と誤判定するのは依存漏れに直結するので、
/// 正規化の責務は突き合わせの直前に置く。
pub fn unused_headers(
    candidates: &BTreeSet<PathBuf>,
    needed: &BTreeSet<PathBuf>,
) -> BTreeSet<PathBuf> {
    candidates.difference(needed).cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(items: &[&str]) -> BTreeSet<PathBuf> {
        items.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn parses_single_rule_with_continuations() {
        let output = "main.o: main.cpp /usr/include/stdio.h \\\n /usr/include/c++/14/iostream";
        assert_eq!(
            parse_prerequisites(output),
            paths(&[
                "main.cpp",
                "/usr/include/stdio.h",
                "/usr/include/c++/14/iostream",
            ])
        );
    }

    #[test]
    fn excludes_the_target() {
        // ':' 左のターゲット (main.o) は前提条件ではないので含まれない。
        let prerequisites = parse_prerequisites("main.o: main.cpp atcoder/dsu.hpp");
        assert!(!prerequisites.contains(&PathBuf::from("main.o")));
        assert!(prerequisites.contains(&PathBuf::from("atcoder/dsu.hpp")));
    }

    #[test]
    fn unions_prerequisites_across_multiple_rules() {
        // 複数入力を 1 度の -M で渡すと複数ルールが並ぶ。和集合を取る。
        let output = "a.o: a.hpp /inc/x.h\nb.o: b.hpp /inc/y.h";
        assert_eq!(
            parse_prerequisites(output),
            paths(&["a.hpp", "/inc/x.h", "b.hpp", "/inc/y.h"])
        );
    }

    #[test]
    fn restores_escaped_spaces_in_paths() {
        let prerequisites = parse_prerequisites("a.o: /home/My\\ Docs/lib.hpp");
        assert!(prerequisites.contains(&PathBuf::from("/home/My Docs/lib.hpp")));
    }

    #[test]
    fn keeps_windows_path_backslashes_intact() {
        // MinGW g++ は Windows パスの `\` をエスケープせずそのまま出す。区切りを食うとドライブ
        // パスが壊れ、必要ヘッダーの取りこぼし = 使用ヘッダーの誤削除に直結する。
        let prerequisites = parse_prerequisites(r"used.o: C:\Users\me\lib\used.hpp D:/other/x.h");
        assert!(prerequisites.contains(&PathBuf::from(r"C:\Users\me\lib\used.hpp")));
        assert!(prerequisites.contains(&PathBuf::from("D:/other/x.h")));
    }

    #[test]
    fn ignores_lines_without_colon() {
        assert!(parse_prerequisites("\n   \nnot a rule\n").is_empty());
    }

    #[test]
    fn unused_is_candidates_minus_needed() {
        let candidates = paths(&["/lib/used.hpp", "/lib/dead.hpp", "/lib/also_dead.hpp"]);
        let needed = paths(&["/lib/used.hpp", "/sys/extra.h"]);
        assert_eq!(
            unused_headers(&candidates, &needed),
            paths(&["/lib/dead.hpp", "/lib/also_dead.hpp"])
        );
    }

    #[test]
    fn nothing_unused_when_all_candidates_are_needed() {
        let headers = paths(&["/lib/a.hpp", "/lib/b.hpp"]);
        assert!(unused_headers(&headers, &headers).is_empty());
    }
}
