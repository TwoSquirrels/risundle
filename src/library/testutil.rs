//! 登録まわりのテスト補助。registry (登録処理) と commands/library (受け口) の両テストが、
//! 同じ手順でソースツリーとストアを組み立てられるよう共有する。

use std::fs;

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

/// 登録済みライブラリの tags.json を、現行と異なる schema_version に書き換える (移行の検証用)。
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
