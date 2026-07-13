//! risundle 本体の更新チェック。`library` サブコマンド実行時にだけ発動し (バンドルの実行経路には
//! 一切絡めない)、crates.io の最新安定版と現在のバージョンを比べて新しい方が出ていれば知らせる。
//!
//! ネットワーク不通・タイムアウト・レスポンスの形式不備など、ユーザーが対処しようのない失敗は
//! エラーにも警告にもせず黙って諦める (何もしないのと同じ結果にする)。`$LOCAL/latest_version_cache.json`
//! に前回確認したバージョンと確認時刻をキャッシュし、鮮度が保たれている間は再度問い合わせない。

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

    let command = if has_cargo_install_update() {
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
fn has_cargo_install_update() -> bool {
    let path_var = std::env::var_os("PATH").unwrap_or_default();
    find_in_path(Path::new("cargo-install-update"), &path_var).is_some()
}
