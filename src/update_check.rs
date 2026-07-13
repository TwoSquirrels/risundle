//! risundle 本体の更新チェック。`library` サブコマンド実行時にだけ発動し (バンドルの実行経路には
//! 一切絡めない)、crates.io の最新安定版と現在のバージョンを比べて新しい方が出ていれば知らせる。
//!
//! ネットワーク不通・タイムアウト・レスポンスの形式不備など、ユーザーが対処しようのない失敗は
//! エラーにも警告にもせず黙って諦める (何もしないのと同じ結果にする)。`$LOCAL/latest_version_cache.json`
//! に前回確認したバージョンと確認時刻をキャッシュし、鮮度が保たれている間は再度問い合わせない。

use std::ffi::OsStr;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Result, anyhow};
use semver::Version;
use serde::{Deserialize, Serialize};

use crate::fs::which::find_in_path;
use crate::library::local::LocalStore;

const CRATE_NAME: &str = "risundle";
const DEFAULT_BASE_URL: &str = "https://crates.io";
const CACHE_TTL: Duration = Duration::from_hours(24);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Serialize, Deserialize)]
struct Cache {
    checked_at: u64,
    latest: String,
}

#[derive(Deserialize)]
struct CratesIoResponse {
    #[serde(rename = "crate")]
    krate: CrateInfo,
}

#[derive(Deserialize)]
struct CrateInfo {
    max_stable_version: Option<String>,
}

/// 新しいバージョンが出ていれば、標準エラーへ一言案内する。
///
/// `RISUNDLE_NO_UPDATE_CHECK` が設定されている場合は何もしない。E2E テストが `library`
/// サブコマンドを叩くたびに本物の crates.io へ問い合わせてしまうのを避けるための、
/// `RISUNDLE_DATA_HOME` と同種の環境変数によるオプトアウト。
pub fn check(store: &LocalStore) {
    if std::env::var_os("RISUNDLE_NO_UPDATE_CHECK").is_some() {
        return;
    }
    if let Some(message) = update_message(store, DEFAULT_BASE_URL) {
        eprintln!("{message}");
    }
}

/// `base_url` を引数で受けるのは、テストで実際の crates.io の代わりにローカルの偽サーバーへ
/// 差し替えるため (トレイト/DI は導入せず、この関数境界だけで済ませる)。
fn update_message(store: &LocalStore, base_url: &str) -> Option<String> {
    let cache_path = store.latest_version_cache_json();
    let latest = if let Some(latest) = cached_latest_version(&cache_path) {
        latest
    } else {
        let latest = fetch_latest_version(base_url).ok()?;
        // キャッシュへの書き込みに失敗しても、通知そのものは続行してよい (次回また問い合わせるだけ)。
        let _ = write_cache(&cache_path, &latest);
        latest
    };

    let current = Version::parse(env!("CARGO_PKG_VERSION")).ok()?;
    let latest = Version::parse(&latest).ok()?;
    if latest <= current {
        return None;
    }

    let path_var = std::env::var_os("PATH").unwrap_or_default();
    let command = if has_cargo_install_update(&path_var) {
        "cargo install-update risundle"
    } else {
        "cargo install risundle --force"
    };
    Some(format!(
        "note: a newer risundle version is available ({current} -> {latest}); run `{command}` to upgrade"
    ))
}

/// キャッシュが有効期限内なら、確認済みの最新バージョンを返す。ファイルが無い・壊れている・
/// 期限切れのいずれも「キャッシュ無し」として扱い、エラーにはしない (純粋なキャッシュなので、
/// 読めなければ黙って作り直すだけでよい)。
fn cached_latest_version(cache_path: &Path) -> Option<String> {
    let content = std::fs::read_to_string(cache_path).ok()?;
    let cache: Cache = serde_json::from_str(&content).ok()?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
    (now.saturating_sub(cache.checked_at) < CACHE_TTL.as_secs()).then_some(cache.latest)
}

fn write_cache(cache_path: &Path, latest: &str) -> Result<()> {
    let checked_at = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let cache = Cache {
        checked_at,
        latest: latest.to_owned(),
    };
    if let Some(parent) = cache_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(cache_path, serde_json::to_string(&cache)?)?;
    Ok(())
}

/// `{base_url}/api/v1/crates/risundle` から `max_stable_version` (プレリリースを除いた最新版) を取得する。
fn fetch_latest_version(base_url: &str) -> Result<String> {
    let url = format!("{base_url}/api/v1/crates/{CRATE_NAME}");
    // crates.io は User-Agent の無いリクエストを弾く。
    let user_agent = format!(
        "risundle/{} (+https://github.com/TwoSquirrels/risundle)",
        env!("CARGO_PKG_VERSION")
    );
    let body = ureq::get(&url)
        .header("User-Agent", &user_agent)
        .config()
        .timeout_global(Some(REQUEST_TIMEOUT))
        .build()
        .call()?
        .body_mut()
        .read_to_string()?;
    let response: CratesIoResponse = serde_json::from_str(&body)?;
    response
        .krate
        .max_stable_version
        .ok_or_else(|| anyhow!("crates.io response for {CRATE_NAME} has no max_stable_version"))
}

/// PATH 上に `cargo-install-update` (cargo-update パッケージが入れるバイナリ) があるか。あれば
/// README で案内している `cargo install-update` を、無ければ素の cargo だけで完結する
/// `cargo install --force` を案内する (pnpm 等が環境に応じて更新コマンドを変えるのに倣う)。
fn has_cargo_install_update(path_var: &OsStr) -> bool {
    find_in_path(Path::new("cargo-install-update"), path_var).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use tempfile::TempDir;

    /// 1回のリクエストにつき1つの JSON レスポンスを返す偽サーバーを起動し、ベース URL と、
    /// 実際に受けたリクエスト数のカウンタを返す。`compiler::testutil::fake_compiler` (偽
    /// コンパイラのシェルスクリプト) と同じ発想の、テスト専用の軽量スタブ。
    fn mock_crates_io(responses: Vec<String>) -> (String, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock server");
        let addr = listener.local_addr().expect("read mock server addr");
        let hits = Arc::new(AtomicUsize::new(0));
        let hits_in_thread = Arc::clone(&hits);
        std::thread::spawn(move || {
            for body in responses {
                let Ok((mut stream, _)) = listener.accept() else {
                    return;
                };
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf); // リクエスト内容は見ず読み捨てる
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes());
                hits_in_thread.fetch_add(1, Ordering::SeqCst);
            }
        });
        (format!("http://{addr}"), hits)
    }

    fn crates_io_body(max_stable_version: &str) -> String {
        format!(r#"{{"crate":{{"max_stable_version":"{max_stable_version}"}}}}"#)
    }

    #[test]
    fn update_message_reports_when_a_newer_version_is_available() {
        let (base_url, _hits) = mock_crates_io(vec![crates_io_body("999.0.0")]);
        let local = TempDir::new().unwrap();
        let store = LocalStore::with_root(local.path());

        let message = update_message(&store, &base_url).expect("newer version should be found");
        assert!(message.contains("999.0.0"), "{message}");
        assert!(message.contains(env!("CARGO_PKG_VERSION")), "{message}");
        assert!(message.contains("cargo install"), "{message}");
    }

    #[test]
    fn update_message_is_silent_when_already_on_the_latest_version() {
        let (base_url, _hits) = mock_crates_io(vec![crates_io_body(env!("CARGO_PKG_VERSION"))]);
        let local = TempDir::new().unwrap();
        let store = LocalStore::with_root(local.path());

        assert!(update_message(&store, &base_url).is_none());
    }

    #[test]
    fn update_message_uses_the_cache_and_avoids_a_second_request() {
        let (base_url, hits) = mock_crates_io(vec![crates_io_body("999.0.0")]);
        let local = TempDir::new().unwrap();
        let store = LocalStore::with_root(local.path());

        assert!(update_message(&store, &base_url).is_some());
        // 2 回目はキャッシュ命中のはずで、偽サーバーは 1 レスポンスしか用意していない。もし
        // キャッシュが効かず再度問い合わせれば、サーバーは既に閉じているため None になり失敗する。
        assert!(update_message(&store, &base_url).is_some());
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn update_message_is_silent_on_connection_failure() {
        let local = TempDir::new().unwrap();
        let store = LocalStore::with_root(local.path());

        // 何も listen していないはずのポートへ向け、接続失敗を再現する。
        assert!(update_message(&store, "http://127.0.0.1:1").is_none());
    }

    #[test]
    fn cached_latest_version_treats_a_missing_file_as_no_cache() {
        let local = TempDir::new().unwrap();
        let store = LocalStore::with_root(local.path());

        assert!(cached_latest_version(&store.latest_version_cache_json()).is_none());
    }

    #[test]
    fn cached_latest_version_treats_a_corrupt_file_as_no_cache() {
        let local = TempDir::new().unwrap();
        let store = LocalStore::with_root(local.path());
        let cache_path = store.latest_version_cache_json();
        std::fs::write(&cache_path, "not json").unwrap();

        assert!(cached_latest_version(&cache_path).is_none());
    }

    #[test]
    fn cached_latest_version_ignores_expired_entries() {
        let local = TempDir::new().unwrap();
        let store = LocalStore::with_root(local.path());
        let cache_path = store.latest_version_cache_json();
        std::fs::create_dir_all(cache_path.parent().unwrap()).unwrap();
        let stale = Cache {
            checked_at: 0,
            latest: "1.0.0".to_owned(),
        };
        std::fs::write(&cache_path, serde_json::to_string(&stale).unwrap()).unwrap();

        assert!(cached_latest_version(&cache_path).is_none());
    }

    #[test]
    fn cached_latest_version_returns_fresh_entries() {
        let local = TempDir::new().unwrap();
        let store = LocalStore::with_root(local.path());
        let cache_path = store.latest_version_cache_json();
        std::fs::create_dir_all(cache_path.parent().unwrap()).unwrap();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let fresh = Cache {
            checked_at: now,
            latest: "1.2.3".to_owned(),
        };
        std::fs::write(&cache_path, serde_json::to_string(&fresh).unwrap()).unwrap();

        assert_eq!(cached_latest_version(&cache_path).as_deref(), Some("1.2.3"));
    }

    #[test]
    fn has_cargo_install_update_detects_the_binary_in_path() {
        let temp = TempDir::new().unwrap();
        std::fs::write(temp.path().join("cargo-install-update"), "").unwrap();

        assert!(has_cargo_install_update(temp.path().as_os_str()));
    }

    #[test]
    fn has_cargo_install_update_is_false_when_absent() {
        let temp = TempDir::new().unwrap();

        assert!(!has_cargo_install_update(temp.path().as_os_str()));
    }
}
