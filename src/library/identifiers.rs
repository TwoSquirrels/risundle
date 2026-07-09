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
//! だけは例外的に除外する。利用コードに必ず現れて全ヘッダーに紐づき、tree-shaking を無効化して
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
/// tree-shaking が無効化される。namespace 内のメンバは子の再帰で個別に拾うため取りこぼさない。
const NAME_NODES: &[&str] = &["identifier", "type_identifier", "field_identifier"];

/// [`enumerate`] の結果。どちらもキーは `source_root` からの相対パス (`/` 区切り)、値は重複排除・
/// 昇順の名前一覧で、該当する名前を持たないファイルはキー自体を含めない。
pub struct Enumeration {
    /// 各ファイルが定義する識別子名 (tags.json の `files`)。
    pub definitions: BTreeMap<String, Vec<String>>,
    /// 各ファイルの「実装先の型名」(tags.json の `implements`)。クラス外の修飾付き定義
    /// (`X<...>::method`) の修飾側や、明示的特殊化 (`template <> struct T<...>`) の主テンプレート名。
    /// 演算子オーバーロードのように定義識別子が残らない実装ファイルでも、依存を逆引きできるように
    /// する。namespace 修飾 (`void ns::f()`) の namespace 名も混ざるが、namespace 名は定義
    /// 識別子として登録されない ([`NAME_NODES`] 参照) ため逆引きで一致することがなく、無害である。
    pub implements: BTreeMap<String, Vec<String>>,
}

/// `source_root` 以下のソースファイルを走査し、各ファイルが定義する識別子名と実装先の型名を
/// 集約する。対象ファイルの選別は [`source::walk_sources`] が担う。
///
/// ファイルを処理する直前に `on_progress(相対パス)` を呼ぶ。登録に時間がかかるため、呼び出し側が
/// 進捗を表示できるようにする。
pub fn enumerate(source_root: &Path, mut on_progress: impl FnMut(&str)) -> Result<Enumeration> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_cpp::LANGUAGE.into())
        .context("failed to initialize the C++ parser")?;
    let mut result = Enumeration {
        definitions: BTreeMap::new(),
        implements: BTreeMap::new(),
    };
    source::walk_sources(source_root, |relative, content| {
        let slug = relpath::to_slash(relative)?;
        on_progress(&slug);
        let (definitions, implements) = names_in_file(&mut parser, content)
            .with_context(|| format!("failed to extract identifiers from {slug}"))?;
        if !definitions.is_empty() {
            result.definitions.insert(slug.clone(), definitions);
        }
        if !implements.is_empty() {
            result.implements.insert(slug, implements);
        }
        Ok(())
    })?;
    Ok(result)
}

/// 1 ファイルのソースから、(定義された識別子名, 実装先の型名) をそれぞれ重複排除・昇順で返す。
fn names_in_file(parser: &mut Parser, source: &[u8]) -> Result<(Vec<String>, Vec<String>)> {
    let tree = parser
        .parse(source, None)
        .ok_or_else(|| anyhow!("failed to parse the source"))?;
    let mut names = BTreeSet::new();
    let mut implements = BTreeSet::new();
    collect_definitions(tree.root_node(), source, &mut names, &mut implements);
    Ok((
        names.into_iter().collect(),
        implements.into_iter().collect(),
    ))
}

/// 構文木を辿り、各宣言ノードが導入する名前を `names` に集める。`SKIP_DESCENT` のノードには降りない。
///
/// 定義を導入するのは `name` / `declarator` フィールドのみ。`type` (戻り値型・変数型)、`base`
/// (継承元)、`scope` (修飾子) などは既存の名前への参照であり、辿ると使用箇所まで識別子化して
/// 逆引きを汚すため辿らない。`type` フィールド内に書かれた埋め込み型定義 (`struct X {} v;`) は、
/// 全子ノードの再帰訪問で当該 `struct` ノードに到達し、その `name` から拾える。
fn collect_definitions(
    node: Node,
    source: &[u8],
    names: &mut BTreeSet<String>,
    implements: &mut BTreeSet<String>,
) {
    if SKIP_DESCENT.contains(&node.kind()) {
        return;
    }
    for field in ["name", "declarator"] {
        if let Some(child) = node.child_by_field_name(field) {
            collect_leaf(child, source, names, implements);
        }
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_definitions(child, source, names, implements);
    }
}

/// `name` / `declarator` フィールドの子から、末端の識別子名を採用する。
///
/// 宣言子は `pointer_declarator` → `function_declarator` → `identifier` のように入れ子になるため、
/// 同名フィールドを辿り続けて末端へ到達する。`qualified_identifier` は `name` 側のみ辿るため、
/// `Foo::bar` からは `bar` を得る (逆引きはトークン単位で行うため修飾子は不要)。
fn collect_leaf(
    node: Node,
    source: &[u8],
    names: &mut BTreeSet<String>,
    implements: &mut BTreeSet<String>,
) {
    // クラス外の修飾付き定義 (`X<...>::method`) は、修飾側 (`X`) が実装先の型名。定義名の探索とは
    // 独立に記録する。name 側の探索は下のフィールド辿りが担う (入れ子の `a::b::f` も再帰で各段の
    // 修飾側を拾う)。
    if node.kind() == "qualified_identifier"
        && let Some(scope) = node.child_by_field_name("scope")
    {
        collect_implement_target(scope, source, implements);
    }
    // 明示的特殊化 (`template <> struct T<X>` / `template <> void f<X>()`) では定義名の位置に
    // テンプレート名 + 実引数のノードが来る。主テンプレート `T` / `f` は他ファイルで定義されている
    // 見込みが高いので、実装先としても記録する。
    if matches!(
        node.kind(),
        "template_type" | "template_function" | "template_method"
    ) {
        collect_implement_target(node, source, implements);
    }
    let mut descended = false;
    for field in ["name", "declarator"] {
        if let Some(child) = node.child_by_field_name(field) {
            collect_leaf(child, source, names, implements);
            descended = true;
        }
    }
    // ユーザー定義リテラル `operator""_mint` は declarator 経由でここに達するが、接尾辞識別子 (`_mint`)
    // は name/declarator フィールドではなく無名の子として吊り下がる。利用コードに現れるのはこの接尾辞
    // なので子まで降りて拾う。記号演算子 (`operator+` 等) は識別子の子を持たず何も増えない。
    if node.kind() == "operator_name" {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            collect_leaf(child, source, names, implements);
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

/// 実装先の型名ノードから、その基底の名前を採用する。`FormalPowerSeries<mint>` のような
/// テンプレート実引数付きはテンプレート名だけを取り、実引数には降りない (実引数は参照であって
/// 実装先ではない)。
fn collect_implement_target(node: Node, source: &[u8], implements: &mut BTreeSet<String>) {
    match node.kind() {
        "namespace_identifier" | "identifier" | "type_identifier" => {
            if let Ok(text) = node.utf8_text(source) {
                implements.insert(text.to_owned());
            }
        }
        "template_type" | "template_function" | "template_method" => {
            if let Some(name) = node.child_by_field_name("name") {
                collect_implement_target(name, source, implements);
            }
        }
        // decltype や依存名などの複雑な修飾は、実装先を静的に特定できないため拾わない。
        // 拾い漏れてもこの逆引きが効かないだけで、定義識別子による依存検出には影響しない。
        _ => {}
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
            .definitions
            .remove("lib.hpp")
            .unwrap_or_default()
    }

    fn implements_in(source: &str) -> Vec<String> {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("source");
        write_file(&root, "lib.hpp", source);
        enumerate(&root, |_| {})
            .unwrap()
            .implements
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

        let files = enumerate(&source, |_| {}).unwrap().definitions;
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
        // (拾うと tree-shaking が無効化される)。中のメンバ (dsu) は拾う。
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
    fn registers_user_defined_literal_suffix() {
        // UDL `operator""_mint` の接尾辞 _mint を登録する。auto 多用で型名が現れず接尾辞だけが
        // 依存の起点になるケースで、検出側 (998244353_mint → _mint) と名前が一致して逆引きが成立する。
        let names = names_in(
            "struct mint {}; mint operator\"\"_mint(unsigned long long x) { return mint{}; }",
        );
        assert!(names.contains(&"_mint".to_owned()));
        assert!(names.contains(&"mint".to_owned()));
    }

    #[test]
    fn symbolic_operator_adds_no_spurious_name() {
        // 記号演算子は接尾辞識別子を持たないため余計な名前を生まない。
        let names = names_in("struct V { V operator+(V o) { return o; } };");
        assert!(names.contains(&"V".to_owned()));
        assert!(!names.contains(&"operator".to_owned()));
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
    fn out_of_class_definition_records_the_implement_target() {
        let source = "void Foo::bar() { }";
        assert_eq!(names_in(source), vec!["bar".to_owned()]);
        assert_eq!(implements_in(source), vec!["Foo".to_owned()]);
    }

    #[test]
    fn templated_out_of_class_definition_records_the_primary_name() {
        // テンプレート実引数 (mint) は参照であって実装先ではないので拾わない。
        let source = "template <typename mint>\n\
             void FormalPowerSeries<mint>::set_fft() { }";
        assert_eq!(implements_in(source), vec!["FormalPowerSeries".to_owned()]);
    }

    #[test]
    fn operator_only_file_still_gets_an_implement_target() {
        // 演算子だけの実装ファイルは定義識別子が空になるため、実装先の記録だけが依存の手がかりになる。
        let source = "template <typename T>\n\
             FormalPowerSeries<T>& FormalPowerSeries<T>::operator*=(const FormalPowerSeries<T>& r) { return *this; }";
        assert_eq!(names_in(source), Vec::<String>::new());
        assert_eq!(implements_in(source), vec!["FormalPowerSeries".to_owned()]);
    }

    #[test]
    fn explicit_specialization_records_the_primary_template() {
        let source = "template <> struct Trait<int> { static const bool value = true; };";
        assert!(implements_in(source).contains(&"Trait".to_owned()));
    }

    #[test]
    fn nested_qualifiers_record_each_scope() {
        let source = "void outer::Inner::f() { }";
        assert_eq!(
            implements_in(source),
            vec!["Inner".to_owned(), "outer".to_owned()]
        );
    }

    #[test]
    fn in_class_definitions_have_no_implement_target() {
        let source = "struct V { V operator+(V o) { return o; } void f() { } };";
        assert_eq!(implements_in(source), Vec::<String>::new());
    }

    #[test]
    fn type_references_do_not_become_implement_targets() {
        // 使用側の修飾 (`internal::barrett bt;` や `Trait<T>::value` の参照) は実装先ではない。
        let source = "struct modint { static internal::barrett bt; };\nint x = Trait<int>::value;";
        assert_eq!(implements_in(source), Vec::<String>::new());
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

        let result = enumerate(&source, |_| {}).unwrap();
        assert!(result.definitions.is_empty());
        assert!(result.implements.is_empty());
    }

    #[test]
    fn errors_when_source_root_is_missing() {
        let temp = TempDir::new().unwrap();
        assert!(enumerate(&temp.path().join("nonexistent"), |_| {}).is_err());
    }
}
