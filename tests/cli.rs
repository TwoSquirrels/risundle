//! 軽量 E2E。実バイナリを CLI 経由で叩き、ライブラリ管理の一連 (登録・一覧・詳細・更新・削除)、
//! 主要なエラー経路、そして自作の小さなライブラリでの tree-shaking を検証する。submodule を必要と
//! しない範囲で、CLI の振る舞いを素早く固める。

mod common;

use std::collections::BTreeSet;
use std::process::Stdio;

use assert_cmd::prelude::*;
use common::{Sandbox, compile_and_run};
use predicates::prelude::*;

/// 維持指定する標準ライブラリ ID。バンドル時はこれを `-k` で展開対象から外す。
const STD: &str = "std";

#[test]
fn list_is_empty_when_nothing_is_registered() {
    let sandbox = Sandbox::bare();
    sandbox
        .risundle()
        .args(["library", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("no libraries are registered"));
}

#[test]
fn add_then_list_and_show_reflect_the_library() {
    let sandbox = Sandbox::bare();
    let source = sandbox.write("mylib/algo.hpp", "#pragma once\nstruct Algo {};\n");
    let lib_root = source.parent().unwrap();

    sandbox
        .risundle()
        .args(["library", "add", "mylib"])
        .arg(lib_root)
        .assert()
        .success()
        .stdout(predicate::str::contains("registered library `mylib`"));

    // list はタブ区切りで `<id>\t<kind>\t<path>` を保つ。
    sandbox
        .risundle()
        .args(["library", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("mylib\tlibrary\t"));

    // show -v は定義済み識別子まで出す。
    sandbox
        .risundle()
        .args(["library", "show", "mylib", "-v"])
        .assert()
        .success()
        .stdout(predicate::str::contains("algo.hpp"))
        .stdout(predicate::str::contains("Algo"));
}

#[test]
fn add_rejects_invalid_id() {
    let sandbox = Sandbox::bare();
    let source = sandbox.write("lib/a.hpp", "int a;");

    sandbox
        .risundle()
        .args(["library", "add", "../evil"])
        .arg(source.parent().unwrap())
        .assert()
        .failure()
        .stderr(predicate::str::contains("not allowed"));
}

#[test]
fn add_rejects_missing_path() {
    let sandbox = Sandbox::bare();
    sandbox
        .risundle()
        .args(["library", "add", "ghost", "/no/such/path"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("include path"));
}

#[test]
fn add_rejects_duplicate_registration() {
    let sandbox = Sandbox::bare();
    let lib_root = sandbox.write("lib/a.hpp", "int a;");
    let lib_root = lib_root.parent().unwrap();

    sandbox
        .risundle()
        .args(["library", "add", "dup"])
        .arg(lib_root)
        .assert()
        .success();
    sandbox
        .risundle()
        .args(["library", "add", "dup"])
        .arg(lib_root)
        .assert()
        .failure()
        .stderr(predicate::str::contains("already registered"));
}

#[test]
fn add_std_id_is_rejected_in_favor_of_add_std() {
    let sandbox = Sandbox::bare();
    let lib_root = sandbox.write("std/vector", "// fake std header");
    sandbox
        .risundle()
        .args(["library", "add", "std"])
        .arg(lib_root.parent().unwrap())
        .assert()
        .failure()
        .stderr(predicate::str::contains("add-std"));
}

#[test]
fn delete_removes_the_library() {
    let sandbox = Sandbox::bare();
    let lib_root = sandbox.write("lib/a.hpp", "int a;");
    let lib_root = lib_root.parent().unwrap();

    sandbox
        .risundle()
        .args(["library", "add", "temp"])
        .arg(lib_root)
        .assert()
        .success();
    sandbox
        .risundle()
        .args(["library", "delete", "temp"])
        .assert()
        .success();
    sandbox
        .risundle()
        .args(["library", "show", "temp"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not registered"));
}

#[test]
fn update_picks_up_new_files() {
    let sandbox = Sandbox::bare();
    let first = sandbox.write("lib/a.hpp", "#pragma once\nstruct A {};\n");
    let lib_root = first.parent().unwrap().to_path_buf();

    sandbox
        .risundle()
        .args(["library", "add", "growing"])
        .arg(&lib_root)
        .assert()
        .success();

    sandbox.write("lib/b.hpp", "#pragma once\nstruct B {};\n");
    sandbox
        .risundle()
        .args(["library", "update", "growing"])
        .assert()
        .success();

    sandbox
        .risundle()
        .args(["library", "show", "growing", "-v"])
        .assert()
        .success()
        .stdout(predicate::str::contains("b.hpp"))
        .stdout(predicate::str::contains("B"));
}

/// all.hpp が used/unused を両方束ねるが main は Used だけ使う、という小さなライブラリ
/// `mylib` を登録し、main.cpp を用意した sandbox を返す。tree-shaking の有無で未使用ヘッダーの
/// 残り方が反転することを対で検証するための共通 fixture。
///
/// メソッド名まで重ならないようにする。逆引きは定義識別子の一致で行うため、`value()` を共有すると
/// 未使用ヘッダーまで巻き込まれてしまう (それ自体は正しい挙動)。
fn used_and_unused_library() -> Sandbox {
    let sandbox = Sandbox::new();
    sandbox.write(
        "mylib/used.hpp",
        "#pragma once\nstruct Used { int used_value() const { return 42; } };\n",
    );
    sandbox.write(
        "mylib/unused.hpp",
        "#pragma once\nstruct Unused { int unused_value() const { return -1; } };\n",
    );
    let all = sandbox.write(
        "mylib/all.hpp",
        "#pragma once\n#include <used.hpp>\n#include <unused.hpp>\n",
    );
    let lib_root = all.parent().unwrap().to_path_buf();

    sandbox
        .risundle()
        .args(["library", "add", "mylib"])
        .arg(&lib_root)
        .assert()
        .success();

    sandbox.write(
        "main.cpp",
        "#include <cstdio>\n#include <all.hpp>\n\
        int main() { Used u; std::printf(\"%d\\n\", u.used_value()); return 0; }\n",
    );

    sandbox
}

/// sandbox 内で `risundle <args> main.cpp` を実行し、標準出力を返す。
fn run_bundle(sandbox: &Sandbox, args: &[&str]) -> String {
    let output = sandbox
        .bundle_command()
        .args(args)
        .arg("main.cpp")
        .output()
        .expect("run bundle");
    assert!(
        output.status.success(),
        "bundle failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("utf-8")
}

/// ソース中の `struct` 定義行の集合。出力どうしの包含関係を比べるのに使う。
fn struct_defs(src: &str) -> BTreeSet<&str> {
    src.lines()
        .filter(|line| line.contains("struct "))
        .collect()
}

#[test]
fn bundle_drops_unused_headers_and_keeps_used_ones() {
    let sandbox = used_and_unused_library();

    let bundled = run_bundle(&sandbox, &["-k", STD]);

    assert!(bundled.contains("struct Used"), "使用ヘッダーは残るべき");
    assert!(
        !bundled.contains("struct Unused"),
        "未使用ヘッダーは削られるべき"
    );
    assert_eq!(compile_and_run(&sandbox, &bundled).trim(), "42");
}

#[test]
fn bundle_no_tree_shaking_keeps_unused_headers() {
    let sandbox = used_and_unused_library();

    let shaken = run_bundle(&sandbox, &["-k", STD]);
    let expanded = run_bundle(&sandbox, &["-k", STD, "--no-tree-shaking"]);

    // 無効版は有効版に残る定義をすべて含む上位集合で、さらに未使用の Unused も残す。
    assert!(
        struct_defs(&shaken).is_subset(&struct_defs(&expanded)),
        "tree-shaking 有効版の定義は無効版に包含されるべき"
    );
    assert!(
        expanded.contains("struct Unused"),
        "tree-shaking 無効時は未使用ヘッダーも残るべき"
    );
    assert_eq!(compile_and_run(&sandbox, &expanded).trim(), "42");
}

#[test]
fn bundle_keeps_operator_implementation_files() {
    // 宣言 (vec2.hpp) と演算子の実装 (vec2-ops.hpp) が分かれたライブラリ。実装側は定義識別子を
    // 持たないため、実装先の型名 (implements) の逆引きだけが依存の手がかりになる。
    let sandbox = Sandbox::new();
    sandbox.write(
        "mylib/vec2.hpp",
        "#pragma once\nstruct Vec2 { int x, y; Vec2 operator+(const Vec2& r) const; };\n",
    );
    sandbox.write(
        "mylib/vec2-ops.hpp",
        "#pragma once\n#include <vec2.hpp>\n\
        Vec2 Vec2::operator+(const Vec2& r) const { return {x + r.x, y + r.y}; }\n",
    );
    let unused = sandbox.write(
        "mylib/unused.hpp",
        "#pragma once\nstruct Unused { int unused_value() const { return 1; } };\n",
    );
    let lib_root = unused.parent().unwrap().to_path_buf();
    sandbox
        .risundle()
        .args(["library", "add", "mylib"])
        .arg(&lib_root)
        .assert()
        .success();

    sandbox.write(
        "main.cpp",
        "#include <cstdio>\n#include <vec2.hpp>\n#include <vec2-ops.hpp>\n#include <unused.hpp>\n\
        int main() { Vec2 a{1, 2}, b{3, 4}; Vec2 c = a + b; std::printf(\"%d\\n\", c.x + c.y); return 0; }\n",
    );

    let bundled = run_bundle(&sandbox, &["-k", STD]);

    // 演算子の使用 (a + b) は識別子として検出できないが、実装ファイルは残るべき。実装が消えた
    // 場合は宣言だけでコンパイルは通り、undefined reference としてリンクで初めて顕在化するため、
    // リンクまで行う compile_and_run で検証する。
    assert!(
        bundled.contains("Vec2 Vec2::operator+"),
        "実装ファイルが tree-shaking で消えてはいけない"
    );
    assert!(
        !bundled.contains("struct Unused"),
        "実装ファイルの救済で無関係な未使用ヘッダーまで残してはいけない"
    );
    assert_eq!(compile_and_run(&sandbox, &bundled).trim(), "10");
}

#[test]
fn bundle_keeps_transitively_required_headers() {
    let sandbox = Sandbox::new();
    // mid.hpp は base.hpp を必要とする。main は Mid だけ使うが、Base も残らねばならない。
    let base = sandbox.write(
        "mylib/base.hpp",
        "#pragma once\nstruct Base { int base_value() const { return 3; } };\n",
    );
    let lib_root = base.parent().unwrap().to_path_buf();
    sandbox.write(
        "mylib/mid.hpp",
        "#pragma once\n#include <base.hpp>\n\
        struct Mid { Base b; int mid_value() const { return b.base_value(); } };\n",
    );
    sandbox.write(
        "mylib/extra.hpp",
        "#pragma once\nstruct Extra { int extra_value() const { return -1; } };\n",
    );

    sandbox
        .risundle()
        .args(["library", "add", "mylib"])
        .arg(&lib_root)
        .assert()
        .success();

    // mid と extra を両方 include するが、使うのは Mid だけ。
    let main = "#include <cstdio>\n#include <mid.hpp>\n#include <extra.hpp>\n\
        int main() { Mid m; std::printf(\"%d\\n\", m.mid_value()); return 0; }\n";
    sandbox.write("main.cpp", main);

    let bundled = run_bundle(&sandbox, &["-k", STD]);

    assert!(bundled.contains("struct Mid"), "使用ヘッダーは残るべき");
    assert!(
        bundled.contains("struct Base"),
        "推移的に必要なヘッダーは残るべき"
    );
    assert!(
        !bundled.contains("struct Extra"),
        "未使用ヘッダーは削られるべき"
    );
    assert_eq!(compile_and_run(&sandbox, &bundled).trim(), "3");
}

#[test]
fn bundle_ignores_identifiers_in_comments_and_strings() {
    let sandbox = Sandbox::new();
    // メソッド名を分け、識別子の逆引き衝突を避ける (両者が `value()` を共有すると巻き込まれる)。
    let real = sandbox.write(
        "mylib/real.hpp",
        "#pragma once\nstruct Real { int real_value() const { return 5; } };\n",
    );
    let lib_root = real.parent().unwrap().to_path_buf();
    sandbox.write(
        "mylib/ghost.hpp",
        "#pragma once\nstruct Ghost { int ghost_value() const { return 9; } };\n",
    );

    sandbox
        .risundle()
        .args(["library", "add", "mylib"])
        .arg(&lib_root)
        .assert()
        .success();

    // ghost.hpp を include しているが、Ghost はコメントと文字列の中にしか現れない。プリプロセスは
    // `-C` でコメントを残すため、字句解析でコメント/文字列を飛ばせていなければ Ghost を使用と誤検出し、
    // ghost.hpp が残ってしまう。
    let main = "#include <cstdio>\n#include <real.hpp>\n#include <ghost.hpp>\n\
        // Ghost ghost_value はここで言及されるだけで使わない。\n\
        const char* note = \"Ghost ghost_value\";\n\
        int main() { Real r; std::printf(\"%d\\n\", r.real_value()); (void)note; return 0; }\n";
    sandbox.write("main.cpp", main);

    let bundled = run_bundle(&sandbox, &["-k", STD]);

    assert!(bundled.contains("struct Real"), "使用ヘッダーは残るべき");
    assert!(
        !bundled.contains("struct Ghost"),
        "コメント/文字列内の言及で未使用ヘッダーを巻き込んではいけない"
    );
    assert_eq!(compile_and_run(&sandbox, &bundled).trim(), "5");
}

#[test]
fn bundle_passes_options_after_double_dash_to_compiler() {
    let sandbox = Sandbox::new();
    sandbox.write(
        "main.cpp",
        "#ifdef RISUNDLE_TEST_ANSWER\nint main() { return RISUNDLE_TEST_ANSWER; }\n\
        #else\n#error RISUNDLE_TEST_ANSWER is not defined\n#endif\n",
    );

    // -D が届かなければ #error でプリプロセスごと失敗するので、成功 = 受け渡しの証明。
    let output = sandbox
        .bundle_command()
        .args(["-k", STD, "main.cpp", "--", "-DRISUNDLE_TEST_ANSWER=0"])
        .output()
        .expect("run bundle");
    assert!(
        output.status.success(),
        "bundle failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let bundled = String::from_utf8(output.stdout).expect("utf-8");
    assert!(
        bundled.contains("return 0"),
        "-D で定義したマクロが展開されるべき"
    );
}

#[test]
fn embed_includes_original_source_as_comment() {
    let sandbox = Sandbox::new();
    let main = "int main() { return 0; }\n";
    sandbox.write("main.cpp", main);

    sandbox
        .bundle_command()
        .args(["-k", STD, "-e", "main.cpp"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--- original source ---"))
        .stdout(predicate::str::contains("// int main() { return 0; }"));
}

#[test]
fn broken_pipe_while_writing_output_does_not_panic() {
    // 出力先パイプを OS のバッファ (Linux で既定 64KiB) より前に閉じ、`| head` のような早期打ち切りを
    // 再現する。main.cpp 自身をコメントで肥大化させ、ライブラリや特定コンパイラの機能に頼らずに
    // 出力サイズを稼ぐ。
    let sandbox = Sandbox::new();
    let filler = "// filler\n".repeat(20_000);
    sandbox.write("main.cpp", &format!("{filler}int main() {{ return 0; }}\n"));

    let mut child = sandbox
        .bundle_command()
        .args(["-k", STD, "main.cpp"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn risundle");

    drop(child.stdout.take());
    let output = child.wait_with_output().expect("wait for risundle");

    assert!(
        !String::from_utf8_lossy(&output.stderr).contains("panicked"),
        "broken pipe でパニックしてはいけない: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn bundle_fails_for_missing_input_file() {
    let sandbox = Sandbox::new();
    sandbox
        .bundle_command()
        .args(["-k", STD, "ghost.cpp"])
        .assert()
        .failure();
}

#[test]
fn bundle_detects_library_changes_unless_verification_is_bypassed() {
    let sandbox = Sandbox::new();
    let header = sandbox.write("mylib/algo.hpp", "#pragma once\nstruct Algo {};\n");
    let lib_root = header.parent().unwrap().to_path_buf();

    sandbox
        .risundle()
        .args(["library", "add", "mylib"])
        .arg(&lib_root)
        .assert()
        .success();

    // 登録後にライブラリを書き換えるとハッシュが食い違う。
    sandbox.write("mylib/algo.hpp", "#pragma once\nstruct Algo { int x; };\n");
    sandbox.write("main.cpp", "int main() { return 0; }\n");

    sandbox
        .bundle_command()
        .args(["-k", STD, "main.cpp"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("has changed since registration"));

    // --no-check ならハッシュ検証を飛ばして通る。
    sandbox
        .bundle_command()
        .args(["-k", STD, "--no-check", "main.cpp"])
        .assert()
        .success();

    // --no-tree-shaking は識別子タグを使わないため、検証自体が不要になり通る。
    sandbox
        .bundle_command()
        .args(["-k", STD, "--no-tree-shaking", "main.cpp"])
        .assert()
        .success();
}
