//! ライブラリ各ファイルが定義する識別子名の列挙。tree-sitter-cpp で構文木を作り、宣言ノードが
//! 導入する名前を網羅的に集約する。`tags.json` の `files` の元データであり、バンドル時の逆引き
//! (識別子 → 依存ヘッダー) に用いる。
//!
//! tree-sitter-tags の tags クエリは定義種別ごとに規則を列挙する方式で、`using` エイリアスや
//! `constexpr` 定数、`concept` などを取りこぼす。そこで本モジュールは「宣言ノードの `name` /
//! `declarator` フィールドの末端識別子を辿る」という単一規則で走査し、種別を列挙せずに網羅する。
//! プリプロセスを行わないため、`#include` 先の定義が混ざらず、ファイル単位の列挙が正しく行える。
//!
//! 逆引きで使わないメンバ変数名やマクロ名も拾うが、risundle は「余分に検出する方向に倒せば安全」
//! という設計のため許容する (取りこぼしのみが依存漏れ = コンパイルエラーを招く)。ただし namespace 名
//! だけは例外的に除外する。利用コードに必ず現れて全ヘッダーに紐づき、Tree-Shaking を無効化して
//! しまうためで、過剰検出が「安全」でなくなる唯一のケースである (詳細は [`NAME_NODES`])。

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{Context, Result, anyhow};
use tree_sitter::{Node, Parser};

use crate::fs::{relpath, source};

/// 走査の再帰で降りないノード。降りても定義は得られず、むしろ参照名を定義と誤認する元になる。
///
/// - 局所スコープ (関数本体・引数・テンプレート仮引数): 公開識別子を含まない。
/// - 型参照 (`qualified_identifier` 等): `name` フィールドを持つが既存名への参照であり、降りると
///   型注釈 `internal::barrett bt;` の `barrett` まで拾ってしまう。定義側の修飾名 (`int N::count;`
///   の `count`) は declarator 経由で別途辿るため、降りなくても取りこぼさない。
const SKIP_DESCENT: &[&str] = &[
    "compound_statement",
    "parameter_list",
    "template_parameter_list",
    "qualified_identifier",
    "template_type",
    "template_function",
    "template_method",
    "dependent_name",
];

/// 宣言子を辿った末端で、識別子名として採用するノード種別。
///
/// `namespace_identifier` は意図的に除外する。namespace 名 (`atcoder` 等) は複数ファイルで開かれ、
/// 利用コードにも必ず現れるため、定義として登録すると逆引きでほぼ全ヘッダーが依存と判定され
/// Tree-Shaking が無効化される。namespace 内のメンバは子の再帰で個別に拾うため取りこぼさない。
const NAME_NODES: &[&str] = &["identifier", "type_identifier", "field_identifier"];

/// `source_root` 以下のソースファイルを走査し、各ファイルが定義する識別子名を集約する。
///
/// キーは `source_root` からの相対パス (`/` 区切り)、値は重複排除・昇順の識別子名一覧。
/// 定義を一つも持たないファイル (インクルードのみの拡張子なしファイル等) は結果に含めない。
/// 対象ファイルの選別は [`source::walk_sources`] が担う。
///
/// ファイルを処理する直前に `on_progress(相対パス)` を呼ぶ。登録に時間がかかるため、呼び出し側が
/// 進捗を表示できるようにする。
pub fn enumerate(
    source_root: &Path,
    mut on_progress: impl FnMut(&str),
) -> Result<BTreeMap<String, Vec<String>>> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_cpp::LANGUAGE.into())
        .context("C++ パーサの初期化に失敗しました")?;
    let mut files = BTreeMap::new();
    source::walk_sources(source_root, |relative, content| {
        let slug = relpath::to_slash(relative)?;
        on_progress(&slug);
        let names = definitions_in(&mut parser, content)
            .with_context(|| format!("{slug} の識別子抽出に失敗しました"))?;
        if !names.is_empty() {
            files.insert(slug, names);
        }
        Ok(())
    })?;
    Ok(files)
}

/// 1 ファイルのソースから、定義された識別子名を重複排除・昇順で返す。
fn definitions_in(parser: &mut Parser, source: &[u8]) -> Result<Vec<String>> {
    let tree = parser
        .parse(source, None)
        .ok_or_else(|| anyhow!("構文解析に失敗しました"))?;
    let mut names = BTreeSet::new();
    collect_definitions(tree.root_node(), source, &mut names);
    Ok(names.into_iter().collect())
}

/// 構文木を辿り、各宣言ノードが導入する名前を `names` に集める。`SKIP_DESCENT` のノードには降りない。
///
/// 定義を導入するのは `name` / `declarator` フィールドのみ。`type` (戻り値型・変数型)、`base`
/// (継承元)、`scope` (修飾子) などは既存の名前への参照であり、辿ると使用箇所まで識別子化して
/// 逆引きを汚すため辿らない。`type` フィールド内に書かれた埋め込み型定義 (`struct X {} v;`) は、
/// 全子ノードの再帰訪問で当該 `struct` ノードに到達し、その `name` から拾える。
fn collect_definitions(node: Node, source: &[u8], names: &mut BTreeSet<String>) {
    if SKIP_DESCENT.contains(&node.kind()) {
        return;
    }
    for field in ["name", "declarator"] {
        if let Some(child) = node.child_by_field_name(field) {
            collect_leaf(child, source, names);
        }
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_definitions(child, source, names);
    }
}

/// `name` / `declarator` フィールドの子から、末端の識別子名を採用する。
///
/// 宣言子は `pointer_declarator` → `function_declarator` → `identifier` のように入れ子になるため、
/// 同名フィールドを辿り続けて末端へ到達する。`qualified_identifier` は `name` 側のみ辿るため、
/// `Foo::bar` からは `bar` を得る (逆引きはトークン単位で行うため修飾子は不要)。
fn collect_leaf(node: Node, source: &[u8], names: &mut BTreeSet<String>) {
    let mut descended = false;
    for field in ["name", "declarator"] {
        if let Some(child) = node.child_by_field_name(field) {
            collect_leaf(child, source, names);
            descended = true;
        }
    }
    if !descended
        && NAME_NODES.contains(&node.kind())
        && let Ok(text) = node.utf8_text(source)
    {
        names.insert(text.to_owned());
    }
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

    fn names_in(source: &str) -> Vec<String> {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("source");
        write_file(&root, "lib.hpp", source);
        enumerate(&root, |_| {})
            .unwrap()
            .remove("lib.hpp")
            .unwrap_or_default()
    }

    #[test]
    fn maps_relative_path_to_its_definitions() {
        let temp = TempDir::new().unwrap();
        let source = temp.path().join("source");
        write_file(
            &source,
            "atcoder/segtree.hpp",
            "namespace atcoder { struct segtree {}; }",
        );

        let files = enumerate(&source, |_| {}).unwrap();
        assert!(files["atcoder/segtree.hpp"].contains(&"segtree".to_owned()));
    }

    #[test]
    fn covers_definition_kinds_that_tags_query_misses() {
        // tree-sitter-tags の標準 tags.scm が取りこぼす種別を、自前走査が拾えることを保証する。
        let names = names_in(
            "union U { int a; };
             using Alias = int;
             constexpr int CONST_VAL = 1;
             template<class T> concept Num = true;
             enum Color { Red, Green };
             template<class T> using Ptr = T*;",
        );
        for expected in [
            "U",
            "Alias",
            "CONST_VAL",
            "Num",
            "Color",
            "Red",
            "Green",
            "Ptr",
        ] {
            assert!(names.contains(&expected.to_owned()), "{expected} が漏れた");
        }
    }

    #[test]
    fn ignores_namespace_names() {
        // namespace 名は利用コードに必ず現れ、ほぼ全ヘッダーに紐づくため定義として拾わない
        // (拾うと Tree-Shaking が無効化される)。中のメンバ (dsu) は拾う。
        let names = names_in("namespace atcoder { namespace internal { struct dsu {}; } }");
        assert!(names.contains(&"dsu".to_owned()));
        for ns in ["atcoder", "internal"] {
            assert!(
                !names.contains(&ns.to_owned()),
                "{ns} は namespace 名なので拾うべきでない"
            );
        }
    }

    #[test]
    fn covers_classes_functions_and_methods() {
        let names = names_in("class Foo {}; void bar(); typedef int Word;");
        assert!(names.contains(&"Foo".to_owned()));
        assert!(names.contains(&"bar".to_owned()));
        assert!(names.contains(&"Word".to_owned()));
    }

    #[test]
    fn ignores_type_references_and_qualifiers() {
        // 戻り値型・引数型・継承元・修飾スコープは「既存名への参照」であり定義ではない。
        let names = names_in("struct D : Base { int x; }; Foo make(Baz q); int N::count = 0;");
        assert!(names.contains(&"D".to_owned()));
        assert!(names.contains(&"make".to_owned()));
        assert!(names.contains(&"count".to_owned()));
        for reference in ["Base", "Foo", "Baz", "N"] {
            assert!(
                !names.contains(&reference.to_owned()),
                "{reference} は参照なのに拾われた"
            );
        }
    }

    #[test]
    fn member_with_qualified_type_does_not_leak_the_type_name() {
        // メンバの型注釈に他名前空間の型を使っても、その型名は定義として拾わない (ACL の barrett 混入回帰)。
        let names = names_in("struct modint { static internal::barrett bt; };");
        assert!(names.contains(&"modint".to_owned()));
        assert!(names.contains(&"bt".to_owned()));
        for reference in ["barrett", "internal"] {
            assert!(
                !names.contains(&reference.to_owned()),
                "{reference} は型参照なのに拾われた"
            );
        }
    }

    #[test]
    fn captures_inline_embedded_type_definitions() {
        // type フィールド内に直接書かれた型定義も、全子再帰で拾える。
        let names = names_in("struct X { int a; } var; enum E { Red } e;");
        for expected in ["X", "a", "var", "E", "Red", "e"] {
            assert!(names.contains(&expected.to_owned()), "{expected} が漏れた");
        }
    }

    #[test]
    fn ignores_local_variables_and_parameters() {
        // 関数本体の局所変数や引数名は公開識別子ではないため拾わない。
        let names = names_in("int solve(int width) { int height = width; return height; }");
        assert!(names.contains(&"solve".to_owned()));
        assert!(!names.contains(&"width".to_owned()));
        assert!(!names.contains(&"height".to_owned()));
    }

    #[test]
    fn names_are_deduplicated_and_sorted() {
        // 前方宣言と定義で同名が複数回現れても 1 つにまとまり、昇順で返る。
        let names = names_in("void zeta(); void alpha(); void zeta() {}");
        assert_eq!(names, vec!["alpha".to_owned(), "zeta".to_owned()]);
    }

    #[test]
    fn files_without_definitions_are_omitted() {
        let temp = TempDir::new().unwrap();
        let source = temp.path().join("source");
        // AC Library の拡張子なしファイルのように、インクルードのみで定義を持たない。
        write_file(&source, "atcoder/modint", "#include <atcoder/modint.hpp>");

        assert!(enumerate(&source, |_| {}).unwrap().is_empty());
    }

    #[test]
    fn errors_when_source_root_is_missing() {
        let temp = TempDir::new().unwrap();
        assert!(enumerate(&temp.path().join("nonexistent"), |_| {}).is_err());
    }
}
