//! プリプロセス後のコードからの識別子検出。`<file>` 由来部分を字句解析し、出現する識別子名を
//! 集める。検出した識別子をライブラリの `files` で逆引きして依存ヘッダーを特定するのが目的。
//!
//! 列挙側 ([`crate::library::identifiers`]) が tree-sitter で構文木を作るのに対し、検出側は logos の
//! 字句解析を使う。linemarker 混じりのプリプロセス後テキストには構文解析より字句解析が頑健で、
//! 検出で最優先の「取りこぼし (= 依存漏れ = コンパイルエラー) を避ける」に適う (過剰検出は無害)。
//!
//! 文字列・文字リテラル・コメントは誤検出を避けてスキップする。C++ raw string (`R"..."`) 内は
//! 通常の文字列として途中で切れ、内部が識別子として拾われうるが、過剰検出は無害なので許容する。
//!
//! 逆引きを行うバンドルのパイプラインが消費するまで未使用となるため、dead_code を許可する。
#![allow(dead_code)]

use std::collections::BTreeSet;

use logos::{Lexer, Logos, Skip};

/// 字句解析のトークン。識別子だけを採取し、他は読み飛ばすために定義する。
///
/// 数値リテラルを 1 トークンとして食わせるのは、桁区切り (`1'000'000`) の `'` を char リテラルの
/// 開始と誤認させないため。識別子は数字始まりになり得ないので、両者は先頭文字で排他に分かれる。
#[derive(Logos)]
enum Token {
    #[regex(r"[A-Za-z_][A-Za-z0-9_]*")]
    Identifier,

    #[regex(r"[0-9][A-Za-z0-9_'.]*", logos::skip)]
    Number,

    #[regex(r#""([^"\\]|\\.)*""#, logos::skip)]
    String,

    #[regex(r"'([^'\\]|\\.)*'", logos::skip)]
    Char,

    #[token("//", skip_line_comment)]
    LineComment,

    #[token("/*", skip_block_comment)]
    BlockComment,
}

/// 行コメントを行末まで読み飛ばす。終端の改行は残し (空白として無害)、次トークンから再開する。
fn skip_line_comment(lexer: &mut Lexer<Token>) -> Skip {
    let rest = lexer.remainder();
    lexer.bump(rest.find('\n').unwrap_or(rest.len()));
    Skip
}

/// ブロックコメントを `*/` まで読み飛ばす。logos の正規表現は非貪欲な終端を表しにくいため、
/// 残り文字列から終端を探すコールバックで処理する。終端が無ければ残り全体をコメント扱いにする。
fn skip_block_comment(lexer: &mut Lexer<Token>) -> Skip {
    let rest = lexer.remainder();
    match rest.find("*/") {
        Some(end) => lexer.bump(end + 2),
        None => lexer.bump(rest.len()),
    }
    Skip
}

/// C++ ソース片に現れる識別子名を、重複排除して返す。
///
/// 文字列・文字リテラル・コメント・数値リテラルはスキップする。トークン化に失敗した記号や空白は
/// logos がエラーとして返すため、識別子以外は一律に無視する。
pub fn identifiers(source: &str) -> BTreeSet<String> {
    let mut lexer = Token::lexer(source);
    let mut names = BTreeSet::new();
    while let Some(token) = lexer.next() {
        if matches!(token, Ok(Token::Identifier)) {
            names.insert(lexer.slice().to_owned());
        }
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    fn detect(source: &str) -> BTreeSet<String> {
        identifiers(source)
    }

    fn contains(source: &str, name: &str) -> bool {
        detect(source).contains(name)
    }

    #[test]
    fn collects_distinct_identifiers() {
        let names = detect("int main() { foo(); foo(); bar; }");
        assert!(names.contains("main"));
        assert!(names.contains("foo"));
        assert!(names.contains("bar"));
        // 重複は 1 つにまとまる (foo は 2 回出現)。
        assert_eq!(names.iter().filter(|n| *n == "foo").count(), 1);
    }

    #[test]
    fn captures_members_and_qualifiers_over_detecting() {
        // メンバ・修飾名も個別に拾う (過剰検出は無害)。
        let names = detect("atcoder::segtree<S> t; t.prod(0, n);");
        for name in ["atcoder", "segtree", "S", "t", "prod", "n"] {
            assert!(names.contains(name), "{name} を拾うべき");
        }
    }

    #[test]
    fn skips_line_comment_contents() {
        assert!(!contains("int x; // hidden symbol", "hidden"));
        assert!(contains("int x; // hidden", "x"));
    }

    #[test]
    fn skips_block_comment_contents_across_lines() {
        let source = "before;\n/* hidden\n   stillhidden */\nafter;";
        let names = detect(source);
        assert!(names.contains("before"));
        assert!(names.contains("after"));
        assert!(!names.contains("hidden"));
        assert!(!names.contains("stillhidden"));
    }

    #[test]
    fn skips_string_and_char_literals() {
        let names = detect(r#"puts("hidden text"); char c = 'x';"#);
        assert!(names.contains("puts"));
        assert!(names.contains("c"));
        assert!(!names.contains("hidden"));
        assert!(!names.contains("text"));
    }

    #[test]
    fn skips_escaped_quotes_in_strings() {
        // エスケープされた引用符で文字列が早期終了せず、内部の識別子も漏れない。
        assert!(!contains(r#"f("a\"hidden\"b");"#, "hidden"));
        assert!(contains(r#"f("a\"hidden\"b");"#, "f"));
    }

    #[test]
    fn digit_separator_does_not_swallow_following_identifier() {
        // 桁区切りの ' を char リテラル開始と誤認すると後続の識別子を飲み込む。これを防ぐ。
        let names = detect("long v = 1'000'000; use(symbol);");
        assert!(names.contains("v"));
        assert!(names.contains("use"));
        assert!(names.contains("symbol"));
    }

    #[test]
    fn returns_empty_for_no_identifiers() {
        assert!(detect("123 + 456 * 7; // comment").is_empty());
    }
}
