//! 軽量 E2E。実バイナリを CLI 経由で叩き、ライブラリ管理の一連 (登録・一覧・詳細・更新・削除)、
//! 主要なエラー経路、そして自作の小さなライブラリでの Tree-Shaking を検証する。submodule を必要と
//! しない範囲で、CLI の振る舞いを素早く固める。

mod common;

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

#[test]
fn bundle_drops_unused_headers_and_keeps_used_ones() {
    let sandbox = Sandbox::new();
    // all.hpp が used/unused を両方束ねるが、main は Used だけ使う。
    // メソッド名まで重ならないようにする。逆引きは定義識別子の一致で行うため、`value()` を共有すると
    // 未使用ヘッダーまで巻き込まれてしまう (それ自体は正しい挙動)。
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

    let main = "#include <cstdio>\n#include <all.hpp>\n\
        int main() { Used u; std::printf(\"%d\\n\", u.used_value()); return 0; }\n";
    sandbox.write("main.cpp", main);

    let output = sandbox
        .risundle()
        .args(["-k", STD, "main.cpp"])
        .output()
        .expect("run bundle");
    assert!(output.status.success(), "bundle failed");
    let bundled = String::from_utf8(output.stdout).expect("utf-8");

    assert!(bundled.contains("struct Used"), "使用ヘッダーは残るべき");
    assert!(
        !bundled.contains("struct Unused"),
        "未使用ヘッダーは削られるべき"
    );
    assert_eq!(compile_and_run(&sandbox, &bundled).trim(), "42");
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

    let output = sandbox
        .risundle()
        .args(["-k", STD, "main.cpp"])
        .output()
        .expect("run bundle");
    assert!(output.status.success(), "bundle failed");
    let bundled = String::from_utf8(output.stdout).expect("utf-8");

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

    let output = sandbox
        .risundle()
        .args(["-k", STD, "main.cpp"])
        .output()
        .expect("run bundle");
    assert!(output.status.success(), "bundle failed");
    let bundled = String::from_utf8(output.stdout).expect("utf-8");

    assert!(bundled.contains("struct Real"), "使用ヘッダーは残るべき");
    assert!(
        !bundled.contains("struct Ghost"),
        "コメント/文字列内の言及で未使用ヘッダーを巻き込んではいけない"
    );
    assert_eq!(compile_and_run(&sandbox, &bundled).trim(), "5");
}

#[test]
fn embed_includes_original_source_as_comment() {
    let sandbox = Sandbox::new();
    let main = "int main() { return 0; }\n";
    sandbox.write("main.cpp", main);

    sandbox
        .risundle()
        .args(["-k", STD, "-e", "main.cpp"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--- original source ---"))
        .stdout(predicate::str::contains("// int main() { return 0; }"));
}

#[test]
fn bundle_fails_for_missing_input_file() {
    let sandbox = Sandbox::new();
    sandbox
        .risundle()
        .args(["-k", STD, "ghost.cpp"])
        .assert()
        .failure();
}

#[test]
fn bundle_detects_library_changes_and_no_check_bypasses_it() {
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
        .risundle()
        .args(["-k", STD, "main.cpp"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("has changed since registration"));

    // --no-check ならハッシュ検証を飛ばして通る。
    sandbox
        .risundle()
        .args(["-k", STD, "--no-check", "main.cpp"])
        .assert()
        .success();
}
