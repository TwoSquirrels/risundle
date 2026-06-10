//! 実ライブラリ E2E。submodule で取得した実際の競技プログラミングライブラリを登録し、それらを
//! 使う小さなプログラムをバンドルして g++ でコンパイル・実行する。
//!
//! コンパイルが通ることは「必要なヘッダーが削られていない」ことしか保証しない。余分なヘッダーが残っても
//! `#pragma once` 等でコンパイルは通ってしまうからだ。そこで併せて、使ったデータ構造とは無関係な (だが
//! ライブラリ内には実在する) シンボルがバンドルに混入していないことも確かめ、tree-shaking が実際に
//! 効いていることを担保する。
//!
//! 前提: submodule が取得済みで g++ が利用可能であること (`git submodule update --init`)。

mod common;

use std::path::Path;

use assert_cmd::prelude::*;
use common::{Sandbox, compile_and_run, fixture};

/// 維持指定する標準ライブラリ ID。
const STD: &str = "std";

/// バンドル結果。展開済みソースと、それを g++ でコンパイル・実行した標準出力を持つ。
struct Bundle {
    source: String,
    output: String,
}

impl Bundle {
    /// `used` のシンボルは残り、`unused` のシンボルは混入していないことを確かめる。
    /// `unused` はライブラリ内に実在するものを渡すこと。そうでないと不在の確認が空振りになる。
    fn assert_tree_shaken(&self, used: &[&str], unused: &[&str]) {
        for symbol in used {
            assert!(
                self.source.contains(symbol),
                "使用シンボル `{symbol}` がバンドルから失われている"
            );
        }
        for symbol in unused {
            assert!(
                !self.source.contains(symbol),
                "未使用シンボル `{symbol}` がバンドルに混入している (tree-shaking が効いていない)"
            );
        }
    }
}

/// `lib_path` を `<id>` で登録し、`source` をバンドル・コンパイル・実行する。
fn bundle_with_library(lib_path: &Path, id: &str, source: &str) -> Bundle {
    let sandbox = Sandbox::new();
    sandbox
        .risundle()
        .args(["library", "add", id])
        .arg(lib_path)
        .assert()
        .success();

    sandbox.write("main.cpp", source);
    let output = sandbox
        .risundle()
        .args(["-k", STD, "main.cpp"])
        .output()
        .expect("run bundle");
    assert!(
        output.status.success(),
        "bundle failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let bundled = String::from_utf8(output.stdout).expect("utf-8");
    let run = compile_and_run(&sandbox, &bundled);
    Bundle {
        source: bundled,
        output: run,
    }
}

#[test]
fn ac_library_dsu_bundles_compiles_and_runs() {
    let source = "#include <atcoder/dsu>\n#include <cstdio>\n\
        int main() {\n\
        \x20 atcoder::dsu d(5);\n\
        \x20 d.merge(0, 1);\n\
        \x20 std::printf(\"%d\\n\", (int)d.size(0));\n\
        \x20 return 0;\n\
        }\n";
    let bundle = bundle_with_library(&fixture("ac-library"), "ac-library", source);
    assert_eq!(bundle.output.trim(), "2");
    bundle.assert_tree_shaken(&["dsu"], &["segtree"]);
}

#[test]
fn ac_library_keeps_only_the_structures_in_use() {
    // 同一ライブラリから 2 つの構造 (dsu と fenwick_tree) を使い、使っていない segtree が
    // 混ざらないこと、つまり tree-shaking が構造単位で正しく選別することを確かめる。
    let source = "#include <atcoder/dsu>\n#include <atcoder/fenwicktree>\n#include <cstdio>\n\
        int main() {\n\
        \x20 atcoder::dsu d(5);\n\
        \x20 d.merge(0, 1);\n\
        \x20 atcoder::fenwick_tree<long long> ft(5);\n\
        \x20 ft.add(0, 10);\n\
        \x20 ft.add(3, 5);\n\
        \x20 std::printf(\"%d %lld\\n\", (int)d.size(0), ft.sum(0, 5));\n\
        \x20 return 0;\n\
        }\n";
    let bundle = bundle_with_library(&fixture("ac-library"), "ac-library", source);
    assert_eq!(bundle.output.trim(), "2 15");
    bundle.assert_tree_shaken(&["dsu", "fenwick_tree"], &["segtree"]);
}

#[test]
fn nyaan_union_find_bundles_compiles_and_runs() {
    let source = "#include <bits/stdc++.h>\nusing namespace std;\n\
        #include <data-structure/union-find.hpp>\n\
        int main() {\n\
        \x20 UnionFind uf(5);\n\
        \x20 uf.unite(0, 1);\n\
        \x20 printf(\"%d\\n\", uf.size(0));\n\
        \x20 return 0;\n\
        }\n";
    let bundle = bundle_with_library(&fixture("nyaan"), "nyaan", source);
    assert_eq!(bundle.output.trim(), "2");
    bundle.assert_tree_shaken(&["UnionFind"], &["SparseTable"]);
}

#[test]
fn luzhiled_union_find_bundles_compiles_and_runs() {
    let source = "#include <bits/stdc++.h>\nusing namespace std;\n\
        #include <structure/union-find/union-find.hpp>\n\
        int main() {\n\
        \x20 UnionFind uf(5);\n\
        \x20 uf.unite(0, 1);\n\
        \x20 printf(\"%d\\n\", uf.size(0));\n\
        \x20 return 0;\n\
        }\n";
    let bundle = bundle_with_library(&fixture("luzhiled"), "luzhiled", source);
    assert_eq!(bundle.output.trim(), "2");
    bundle.assert_tree_shaken(&["UnionFind"], &["SegmentTree"]);
}

#[test]
fn kactl_union_find_bundles_compiles_and_runs() {
    // KACTL のヘッダーは `vi` (= vector<int>) 等の typedef を利用側が用意する前提。
    let source = "#include <bits/stdc++.h>\nusing namespace std;\n\
        typedef vector<int> vi;\n\
        #include <data-structures/UnionFind.h>\n\
        int main() {\n\
        \x20 UF uf(5);\n\
        \x20 uf.join(0, 1);\n\
        \x20 printf(\"%d\\n\", uf.size(0));\n\
        \x20 return 0;\n\
        }\n";
    let bundle = bundle_with_library(&fixture("kactl").join("content"), "kactl", source);
    assert_eq!(bundle.output.trim(), "2");
    bundle.assert_tree_shaken(&["UF"], &["Fenwick"]);
}
