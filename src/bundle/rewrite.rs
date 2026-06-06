//! プリプロセス出力の置換。上から 1 行ずつ走査し、(1) 不要ヘッダー由来のコード行を削除し、
//! (2) ダミーの `#pragma RISUNDLE_DUMMY <...>` を `#include <...>` へ復元する。
//!
//! linemarker は「残ったコードがどの元ファイルの何行目か」を示すために保つが、不要なものは残さない:
//!
//! - **コードごと削除されたヘッダーの marker** — 指す先が無くなるので残さない。linemarker は即座に
//!   出力せず保留し、生き残るコード行の直前でのみ吐き出す。領域が丸ごと消えたヘッダーの marker は後続の
//!   marker に上書きされて捨てられる。
//! - **ダミーファイル自身を指す marker** — risundle 内部のダミーを指すだけで無意味なので捨てる。直後が
//!   ダミー pragma 行になっている marker がそれと判る (ダミーは pragma 1 行のみ)。
//!
//! ダミー由来の `#include` も生存コード行の一種として扱い、その直前で保留中の実 marker (例:
//! `#line 5 "main.cpp"`) を出す。これにより復元した `#include` も元ソースでの行位置に正しく紐づく。
//!
//! 残す marker は GCC の linemarker (`# <行> "<ファイル>" <フラグ>`) ではなく、標準の `#line` ディレクティブ
//! (`#line <行> "<ファイル>"`) として出力する。バンドル結果は提出される「ソース」であり、`# <行> ...` は
//! プリプロセッサ「出力」用の GNU 拡張なので、ソースには規格準拠で可搬な `#line` がふさわしい。あわせて
//! 入れ子フラグ (1=進入, 2=復帰, ...) も落とす。平坦化されたバンドルでは進入・復帰の対応が崩れており
//! (ダミーの進入 marker を落とすため)、フラグ付き linemarker を残すと "linemarker ignored due to incorrect
//! nesting" を警告するうえ、`#line` はそもそもフラグを持たない。出所を示すのに要るのは行番号とファイル名だけ。
//!
//! さらに、`#line` の行番号は物理行ごとに自動加算されるため、「同じファイルの、ちょうど次に来る行番号」を
//! 指す `#line` は無操作で冗長になる (例: 復元した `#include` の直後の復帰 marker)。出力済みの presumed 位置を
//! 追い、一致する `#line` は省く。刈り取りで物理行数と元の行番号がズレた箇所でだけ実際に出力され再同期する。
//!
//! 行の出所は [`Tracker`] が追う。刈り取りはヘッダー単位 (linemarker で区切られた領域は出所が同一の
//! ため全削除か全保持の二択) なので、生き残る領域は必ず手前に復帰 linemarker を伴い、保留方式でも
//! 行番号の正確さは保たれる。不要ヘッダーにネストされた別ヘッダーは、出所が切り替わるたびに要否を
//! 独立に判断する。

use crate::bundle::linemarker::{Line, Linemarker, Tracker};
use crate::library::dummy::DUMMY_PRAGMA;

/// プリプロセス結果を走査し、置換済みのコードを返す。
///
/// `is_unused` は出所ファイル (linemarker のパス文字列) が不要ヘッダーかを判定する。不要と判定された
/// 出所のコード行を削除し、その領域だけを指す linemarker やダミーを指す linemarker も併せて落とす。
///
/// `display` は `#line` に出すファイル名を整える (例: ローカル絶対パス → ライブラリ ID 基準の相対
/// パス)。冗長な `#line` の間引きは変換前のパスで判定するため、`display` が別パスを同名へ畳んでも
/// 取りこぼしは起きない。
pub fn rewrite(
    preprocessed: &str,
    is_unused: impl Fn(&str) -> bool,
    display: impl Fn(&str) -> String,
) -> String {
    // 次行を覗いてダミー marker を判定するため、行を一旦集める。
    let lines: Vec<&str> = preprocessed.lines().collect();
    let mut tracker = Tracker::new();
    let mut output = String::new();
    // 直近に観測したが未出力の linemarker。生存コードの直前でのみ `#line` として吐き出す。
    let mut pending: Option<Linemarker> = None;
    // 直前に出した `#line` が示す出所。これを起点に物理行ごとへ presumed 行番号を割り当てる。
    let mut presumed = Presumed::default();
    for (index, &line) in lines.iter().enumerate() {
        match tracker.observe(line) {
            Line::Marker(marker) => {
                // 次行がダミー pragma なら、この marker はダミーファイル自身を指す。保留に回さず捨てる。
                let points_to_dummy = lines
                    .get(index + 1)
                    .copied()
                    .is_some_and(|next| restore_include(next).is_some());
                if !points_to_dummy {
                    pending = Some(marker);
                }
            }
            Line::Code { file } => {
                if file.is_some_and(&is_unused) {
                    continue; // 不要ヘッダー由来のコード行は削除 (保留中の marker も出さない)
                }
                // ダミー pragma は `#include` へ復元する。復元 include も含めどの生存コード行も、出所を
                // 示すため保留中の linemarker をここで確定出力する。ダミー自身を指す marker は上で捨て済み
                // なので、ここで flush される pending は include の出所を指す実 marker (例: `#line 5 "main.cpp"`)。
                flush_marker(&mut output, &mut pending, &mut presumed, &display);
                push_line(&mut output, restore_include(line).as_deref().unwrap_or(line));
                presumed.advance();
            }
        }
    }
    output
}

/// 出力済みコードの presumed 出所 (直前の `#line` を起点に、物理行ごとへ割り当てる行番号)。
#[derive(Default)]
struct Presumed {
    file: Option<String>,
    /// 次に出力する物理行に割り当たる行番号。
    line: u32,
}

impl Presumed {
    fn advance(&mut self) {
        self.line += 1;
    }
}

/// 保留中の marker を `#line` ディレクティブとして確定出力する。ただし既に同じファイルの同じ行に
/// いるなら `#line` は無操作 (物理行の自然な加算で行番号が一致する) なので省く。刈り取りで物理行数が
/// ズレた箇所でだけ実際に出力され、行番号が再同期される。
fn flush_marker(
    output: &mut String,
    pending: &mut Option<Linemarker>,
    presumed: &mut Presumed,
    display: &impl Fn(&str) -> String,
) {
    let Some(marker) = pending.take() else {
        return;
    };
    if presumed.file.as_deref() == Some(marker.file.as_str()) && presumed.line == marker.line {
        return;
    }
    push_line(output, &line_directive(marker.line, &display(&marker.file)));
    presumed.line = marker.line;
    presumed.file = Some(marker.file);
}

fn push_line(output: &mut String, line: &str) {
    output.push_str(line);
    output.push('\n');
}

/// 標準の `#line <行> "<ファイル>"` ディレクティブを組み立てる。入れ子フラグは持たせず、ファイル名は
/// 規格の文字列リテラルとして `\` と `"` をエスケープして引用符で囲む。
fn line_directive(line: u32, file: &str) -> String {
    let escaped = file.replace('\\', "\\\\").replace('"', "\\\"");
    format!("#line {line} \"{escaped}\"")
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

    /// 出所パスをそのまま `#line` に出す表示関数 (変換を検証しないテスト用)。
    fn verbatim(file: &str) -> String {
        file.to_owned()
    }

    #[test]
    fn keeps_linemarkers_as_line_directives() {
        // 入力の GCC linemarker (`# 1 "..."`) は標準の `#line` ディレクティブとして出力する。
        let input = "# 1 \"main.cpp\"\nint main() {}\n";
        assert_eq!(
            rewrite(input, unused(&[]), verbatim),
            "#line 1 \"main.cpp\"\nint main() {}\n"
        );
    }

    #[test]
    fn deletes_unused_header_code_along_with_its_dangling_marker() {
        // dead.hpp を刈ると物理行が詰まり、int x は元の 4 行目に戻すため `#line 4` の再同期が要る。
        let input = "# 1 \"main.cpp\"\nint main() {}\n# 1 \"/lib/dead.hpp\"\nstruct Dead {};\nstruct Dead2 {};\n# 4 \"main.cpp\"\nint x;\n";
        let output = rewrite(input, unused(&["/lib/dead.hpp"]), verbatim);

        assert!(output.contains("int main() {}"));
        assert!(output.contains("int x;"));
        assert!(!output.contains("struct Dead {};")); // 不要ヘッダーのコードは削除
        // コードごと消えた dead.hpp の linemarker も落とす。main.cpp 側の再同期 marker は残る。
        assert!(!output.contains("/lib/dead.hpp"));
        assert!(output.contains("#line 1 \"main.cpp\""));
        assert!(output.contains("#line 4 \"main.cpp\""));
    }

    #[test]
    fn omits_redundant_line_directive() {
        // 物理行の自然な加算で行番号が一致する `#line` は無操作なので出さない。
        // int x が 1 行目 → 次は自動で 2 行目なので、`# 2 "a.cpp"` は冗長。
        let input = "# 1 \"a.cpp\"\nint x;\n# 2 \"a.cpp\"\nint y;\n";
        assert_eq!(
            rewrite(input, unused(&[]), verbatim),
            "#line 1 \"a.cpp\"\nint x;\nint y;\n"
        );
    }

    #[test]
    fn keeps_marker_that_precedes_surviving_code() {
        // 生き残るコードの直前の linemarker は、出所を示すため `#line` として残す。
        let input = "# 1 \"main.cpp\"\nint x;\n# 42 \"main.cpp\"\nint y;\n";
        assert_eq!(
            rewrite(input, unused(&[]), verbatim),
            "#line 1 \"main.cpp\"\nint x;\n#line 42 \"main.cpp\"\nint y;\n"
        );
    }

    #[test]
    fn nested_header_is_judged_independently() {
        // 不要ヘッダー A の途中で必要ヘッダー B に切り替わると、B の行は残る。
        let input = "# 1 \"/lib/A.hpp\"\nstruct A {};\n# 1 \"/lib/B.hpp\"\nstruct B {};\n# 5 \"/lib/A.hpp\"\nstruct A2 {};\n";
        let output = rewrite(input, unused(&["/lib/A.hpp"]), verbatim);

        assert!(!output.contains("struct A {};"));
        assert!(!output.contains("struct A2 {};"));
        assert!(output.contains("struct B {};")); // B は不要指定されていないので残る
    }

    #[test]
    fn restores_dummy_pragma_and_drops_its_internal_marker() {
        let input = "# 1 \"/local/dummy/atcoder/dsu\"\n#pragma RISUNDLE_DUMMY <atcoder/dsu>\n";
        let output = rewrite(input, unused(&[]), verbatim);
        assert_eq!(output, "#include <atcoder/dsu>\n");
        // 内部ダミーパスを指す linemarker は残さない。
        assert!(!output.contains("dummy"));
    }

    #[test]
    fn keeps_leading_file_marker_even_when_first_line_is_a_restored_include() {
        // 先頭が復元 #include でも、バンドル全体の出所 (#line 1 "sol.cpp") は残す。
        let input = "# 1 \"sol.cpp\"\n# 1 \"/local/dummy/iostream\"\n#pragma RISUNDLE_DUMMY <iostream>\n# 2 \"sol.cpp\" 2\nint main() {}\n";
        let output = rewrite(input, unused(&[]), verbatim);
        assert!(output.starts_with("#line 1 \"sol.cpp\"\n#include <iostream>\n"));
        assert!(!output.contains("dummy"));
    }

    #[test]
    fn keeps_real_marker_around_a_restored_include() {
        // ダミーの marker は捨てるが、それを囲む実ファイルの marker は残る。
        let input = "# 1 \"modint.hpp\"\nstruct mint {};\n# 1 \"/local/dummy/cassert\"\n#pragma RISUNDLE_DUMMY <cassert>\n# 5 \"modint.hpp\" 2\nint x;\n";
        let output = rewrite(input, unused(&[]), verbatim);
        assert!(output.contains("#line 1 \"modint.hpp\""));
        assert!(output.contains("#include <cassert>"));
        // 復帰 marker はフラグを剥がして `#line` で残す (平坦化後は入れ子フラグが nesting 警告の元になる)。
        assert!(output.contains("#line 5 \"modint.hpp\"\n"));
        assert!(!output.contains(" 2\n")); // フラグ 2 は残さない
        assert!(!output.contains("dummy"));
    }

    #[test]
    fn emits_line_directives_without_flags() {
        // 進入・復帰・システムヘッダのフラグを剥がし、標準 `#line` でファイル名と行番号だけを残す。
        let input = "# 1 \"main.cpp\"\nint a;\n# 9 \"main.cpp\" 2 3\nint b;\n";
        let output = rewrite(input, unused(&[]), verbatim);
        assert!(output.contains("#line 9 \"main.cpp\"\n"));
        assert!(!output.contains("# 9 \"main.cpp\"")); // GCC linemarker 形式は残さない
        assert!(!output.contains(" 2 3")); // フラグも残さない
    }

    #[test]
    fn flushes_pending_marker_before_a_restored_include_mid_stream() {
        // 先頭以外でも、復元 #include の直前に保留中の実 marker を出す。これが無いと include が
        // 直前コードの presumed 行番号 (ここでは 2) に居座り、本来の出所 main.cpp:5 とズレる。
        let input = "# 1 \"main.cpp\"\nint a;\n# 5 \"main.cpp\"\n# 1 \"/local/dummy/atcoder/dsu\"\n#pragma RISUNDLE_DUMMY <atcoder/dsu>\n# 6 \"main.cpp\" 2\nint b;\n";
        assert_eq!(
            rewrite(input, unused(&[]), verbatim),
            "#line 1 \"main.cpp\"\nint a;\n#line 5 \"main.cpp\"\n#include <atcoder/dsu>\nint b;\n"
        );
    }

    #[test]
    fn normalizes_quoted_dummy_to_angle_include() {
        let output = rewrite(
            "#pragma RISUNDLE_DUMMY \"atcoder/dsu\"\n",
            unused(&[]),
            verbatim,
        );
        assert!(output.contains("#include <atcoder/dsu>"));
    }

    #[test]
    fn leaves_unrelated_pragmas_untouched() {
        let input = "#pragma once\n#pragma GCC optimize(\"O2\")\n";
        assert_eq!(rewrite(input, unused(&[]), verbatim), input);
    }
}
