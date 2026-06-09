//! プリプロセス出力の [linemarker] 解析。`gcc -E` は展開結果に `# 行番号 "ファイル" フラグ...` の
//! 形式で出所を埋め込む。これを解釈して各コード行がどのファイル由来かを追跡し、tree-shaking の
//! 「`<file>` 由来部分だけを見る」「ヘッダー単位で要否を判断する」工程の土台にする。
//!
//! [linemarker]: https://gcc.gnu.org/onlinedocs/cpp/Preprocessor-Output.html

/// 1 本の linemarker。`# <line> "<file>" <flags...>` を解釈した結果。
///
/// `flags` は GCC が付す数値フラグ (1=ファイル開始, 2=復帰, 3=システムヘッダ, 4=暗黙 extern "C")。
/// 出所判定では `file` のみを使うが、将来システムヘッダの区別に使えるよう保持しておく。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Linemarker {
    pub line: u32,
    pub file: String,
    pub flags: Vec<u32>,
}

impl Linemarker {
    /// 1 行を linemarker として解釈する。linemarker でなければ `None`。
    ///
    /// `#pragma` などの linemarker 以外の `#` 始まり行や通常のコード行は `None` を返す。
    /// ファイル名は引用符で囲まれ、空白を含みうるため、行番号の直後の引用符から閉じ引用符までを
    /// 名前とみなす (フラグはその後ろの空白区切り整数)。
    pub fn parse(line: &str) -> Option<Self> {
        // linemarker は必ず "# " で始まり、直後に行番号が来る。"#pragma" 等はここで弾かれる。
        let (number, rest) = line.strip_prefix("# ")?.split_once(' ')?;
        let line = number.parse().ok()?;
        let (file, rest) = parse_quoted(rest.trim_start())?;
        let flags = rest
            .split_whitespace()
            .map(str::parse)
            .collect::<Result<_, _>>()
            .ok()?;
        Some(Self { line, file, flags })
    }
}

/// 引用符で囲まれた文字列を取り出し、閉じ引用符の後ろの残りと共に返す。
///
/// GCC はファイル名中の `"` と `\` をバックスラッシュでエスケープする。この 2 つを復元できれば
/// 実在パスの照合には十分なので、その他のエスケープ列はバックスラッシュを外した文字をそのまま採る。
fn parse_quoted(s: &str) -> Option<(String, &str)> {
    let body = s.strip_prefix('"')?;
    let mut unescaped = String::new();
    let mut chars = body.char_indices();
    while let Some((index, ch)) = chars.next() {
        match ch {
            '\\' => unescaped.push(chars.next()?.1),
            '"' => return Some((unescaped, &body[index + 1..])),
            _ => unescaped.push(ch),
        }
    }
    None // 閉じ引用符が無い
}

/// プリプロセス出力を行ごとに走査し、各コード行の出所ファイルを与える状態機械。
///
/// linemarker 行を観測するたびに「現在の出所」を更新し、続くコード行をその出所に紐づける。
/// 行を 1 つずつ [`Tracker::observe`] に与えて使う。
#[derive(Default)]
pub struct Tracker {
    current: Option<String>,
}

/// [`Tracker::observe`] が返す、1 行の分類結果。
pub enum Line<'a> {
    /// linemarker 行。出力にはそのまま残しつつ、出所更新にのみ使う。
    Marker(Linemarker),
    /// 通常のコード行。`file` は現在の出所 (linemarker 未出現なら `None`)。
    Code { file: Option<&'a str> },
}

impl Tracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// 1 行を観測する。linemarker なら出所を更新して [`Line::Marker`] を、それ以外は現在の出所を
    /// 添えた [`Line::Code`] を返す。
    pub fn observe(&mut self, line: &str) -> Line<'_> {
        match Linemarker::parse(line) {
            Some(marker) => {
                self.current = Some(marker.file.clone());
                Line::Marker(marker)
            }
            None => Line::Code {
                file: self.current.as_deref(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(line: &str) -> Linemarker {
        Linemarker::parse(line).unwrap()
    }

    #[test]
    fn parses_basic_marker_without_flags() {
        assert_eq!(
            parse("# 3 \"main.cpp\""),
            Linemarker {
                line: 3,
                file: "main.cpp".to_owned(),
                flags: vec![],
            }
        );
    }

    #[test]
    fn parses_marker_with_flags() {
        let marker = parse("# 1 \"/usr/include/c++/14/iostream\" 1 3");
        assert_eq!(marker.line, 1);
        assert_eq!(marker.file, "/usr/include/c++/14/iostream");
        assert_eq!(marker.flags, vec![1, 3]);
    }

    #[test]
    fn parses_pseudo_files() {
        for name in ["<built-in>", "<command-line>"] {
            assert_eq!(parse(&format!("# 1 \"{name}\"")).file, name);
        }
    }

    #[test]
    fn keeps_spaces_inside_filename() {
        // パスに空白が含まれても、閉じ引用符までを名前として取り込む。
        assert_eq!(
            parse("# 5 \"/home/My Docs/a.cpp\"").file,
            "/home/My Docs/a.cpp"
        );
    }

    #[test]
    fn unescapes_quote_and_backslash() {
        // GCC は名前中の \ と " をエスケープする。\\ -> \, \" -> "。
        assert_eq!(parse(r#"# 1 "a\\b\"c.h""#).file, "a\\b\"c.h");
    }

    #[test]
    fn rejects_non_linemarkers() {
        for line in [
            "#pragma once",
            "int main() {}",
            "  # 1 \"x\"",  // 行頭スペースは linemarker でない
            "# x \"file\"", // 行番号が数値でない
            "# 1 unquoted", // ファイル名が引用符で囲まれていない
            "# 1 \"unterminated",
            "#",
            "",
        ] {
            assert!(Linemarker::parse(line).is_none(), "{line:?} を弾くべき");
        }
    }

    #[test]
    fn tracker_attributes_code_to_current_file() {
        let mut tracker = Tracker::new();

        assert!(matches!(
            tracker.observe("# 1 \"main.cpp\""),
            Line::Marker(_)
        ));
        assert!(matches!(
            tracker.observe("int main() {}"),
            Line::Code {
                file: Some("main.cpp")
            }
        ));

        // 別ファイルへ切り替わると、以降のコードはそのファイルに紐づく。
        assert!(matches!(
            tracker.observe("# 2 \"lib.hpp\""),
            Line::Marker(_)
        ));
        assert!(matches!(
            tracker.observe("struct S {};"),
            Line::Code {
                file: Some("lib.hpp")
            }
        ));
    }

    #[test]
    fn tracker_reports_none_before_any_marker() {
        let mut tracker = Tracker::new();
        assert!(matches!(
            tracker.observe("// preamble"),
            Line::Code { file: None }
        ));
    }
}
