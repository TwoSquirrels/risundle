//! 登録まわりのテスト補助。registry (登録処理) と commands (受け口) の両テストが、
//! 同じ手順でソースツリー・ストア・偽コンパイラを組み立てられるよう共有する。

use std::fs;
#[cfg(unix)]
use std::path::{Path, PathBuf};

use tempfile::TempDir;

use crate::library::local::LocalStore;
use crate::library::tags::Tags;

pub fn source_with(files: &[(&str, &str)]) -> TempDir {
    let temp = TempDir::new().unwrap();
    for (relative, content) in files {
        let path = temp.path().join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }
    temp
}

pub fn store_in(local: &TempDir) -> LocalStore {
    LocalStore::with_root(local.path())
}

// 偽コンパイラの生成は、依存の終端である compiler 側の補助を再輸出する (library → compiler は
// 許された辺で、逆向きの重複定義を持たずに済む)。
#[cfg(unix)]
pub use crate::compiler::testutil::fake_compiler;

/// `-v` の探索リストに `include_dir` だけを出す偽コンパイラを作る (std 登録の検出経路用)。
#[cfg(unix)]
pub fn fake_compiler_with_includes(dir: &Path, include_dir: &Path) -> PathBuf {
    fake_compiler(
        dir,
        &format!(
            "echo '#include <...> search starts here:' >&2\n\
             echo ' {}' >&2\n\
             echo 'End of search list.' >&2",
            include_dir.display()
        ),
    )
}

/// 登録済みライブラリの `tags.json` を、現行と異なる `schema_version` に書き換える (移行の検証用)。
pub fn downgrade_schema(store: &LocalStore, id: &str) {
    let tags_path = store.tags_json(id).unwrap();
    let old = fs::read_to_string(&tags_path)
        .unwrap()
        .replace("\"schema_version\": 2", "\"schema_version\": 1");
    fs::write(&tags_path, old).unwrap();
    assert!(
        Tags::load(&tags_path).is_err(),
        "旧スキーマは通常読み込みでは弾かれる前提"
    );
}
