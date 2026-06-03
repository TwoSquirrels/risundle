use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

const CONFIG_FILE_NAME: &str = ".risundlerc.toml";

const DEFAULT_COMPILER: &str = "g++";
const DEFAULT_OPTIONS: [&str; 4] = ["-std=gnu++17", "-O2", "-DONLINE_JUDGE", "-DATCODER"];
const DEFAULT_KEEP: [&str; 1] = ["std"];

/// `.risundlerc.toml` を解決して得られる実効的な設定値。
///
/// `.risundlerc.toml` で省略された項目は組み込みデフォルトで補完されるため、
/// 全項目が必ず値を持つ。CLI オプションとの優先順位マージは呼び出し側の責務とする。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub compiler: PathBuf,
    pub options: Vec<String>,
    pub keep: Vec<String>,
    pub embed: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            compiler: PathBuf::from(DEFAULT_COMPILER),
            options: DEFAULT_OPTIONS.iter().map(|&o| o.to_owned()).collect(),
            keep: DEFAULT_KEEP.iter().map(|&id| id.to_owned()).collect(),
            embed: false,
        }
    }
}

/// `<file>` のあるディレクトリからファイルシステムのルートへ向けて親を辿り、
/// 最初に見つかった `.risundlerc.toml` を解決して返す。
///
/// 仕様上、最も近い 1 ファイルのみを採用し、複数ファイル間のマージはしない。
/// どこにも見つからない場合は組み込みデフォルト (`Config::default()`) を返す。
pub fn resolve(start_file: &Path) -> Result<Config> {
    let Some(path) = find_config_file(start_file)? else {
        return Ok(Config::default());
    };
    load(&path)
}

fn find_config_file(start_file: &Path) -> Result<Option<PathBuf>> {
    // ancestors() が機能するよう絶対パス化する。相対パスのままだと親を辿れない。
    let absolute = std::path::absolute(start_file)
        .with_context(|| format!("{} の絶対パス化に失敗しました", start_file.display()))?;
    // 先頭 (ファイル自身) を除いた親ディレクトリ群を、近い順に探索する。
    let found = absolute
        .ancestors()
        .skip(1)
        .map(|dir| dir.join(CONFIG_FILE_NAME))
        .find(|candidate| candidate.is_file());
    Ok(found)
}

fn load(path: &Path) -> Result<Config> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("{} の読み込みに失敗しました", path.display()))?;
    let raw: RawConfig = toml::from_str(&text)
        .with_context(|| format!("{} のパースに失敗しました", path.display()))?;
    Ok(raw.into())
}

/// `.risundlerc.toml` の生のデシリアライズ結果。全項目を省略可能とする。
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    #[serde(default)]
    compiler: RawCompiler,
    #[serde(default)]
    library: RawLibrary,
    #[serde(default)]
    bundle: RawBundle,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCompiler {
    path: Option<PathBuf>,
    options: Option<Vec<String>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLibrary {
    keep: Option<Vec<String>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawBundle {
    embed: Option<bool>,
}

impl From<RawConfig> for Config {
    fn from(raw: RawConfig) -> Self {
        let default = Config::default();
        Self {
            compiler: raw.compiler.path.unwrap_or(default.compiler),
            options: raw.compiler.options.unwrap_or(default.options),
            keep: raw.library.keep.unwrap_or(default.keep),
            embed: raw.bundle.embed.unwrap_or(default.embed),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::fs;

    use tempfile::TempDir;

    #[test]
    fn omitted_fields_fall_back_to_builtin_defaults() {
        let raw: RawConfig = toml::from_str("[bundle]\nembed = true\n").unwrap();
        let config: Config = raw.into();

        assert_eq!(
            config,
            Config {
                embed: true,
                ..Config::default()
            }
        );
    }

    #[test]
    fn specified_fields_override_defaults() {
        let toml = r#"
            [compiler]
            path = "/usr/bin/clang++"
            options = ["-std=gnu++2b", "-O2"]

            [library]
            keep = ["std", "ac-library"]

            [bundle]
            embed = true
        "#;
        let config: Config = toml::from_str::<RawConfig>(toml).unwrap().into();

        assert_eq!(
            config,
            Config {
                compiler: PathBuf::from("/usr/bin/clang++"),
                options: vec!["-std=gnu++2b".to_owned(), "-O2".to_owned()],
                keep: vec!["std".to_owned(), "ac-library".to_owned()],
                embed: true,
            }
        );
    }

    #[test]
    fn unknown_field_is_rejected() {
        let result: Result<RawConfig, _> = toml::from_str("[compiler]\nunknown = 1\n");
        assert!(result.is_err());
    }

    #[test]
    fn resolve_returns_builtin_defaults_when_no_config_found() {
        let temp = TempDir::new().unwrap();
        let source = temp.path().join("main.cpp");
        fs::write(&source, "int main() {}").unwrap();

        assert_eq!(resolve(&source).unwrap(), Config::default());
    }

    #[test]
    fn resolve_picks_nearest_config_without_merging() {
        let temp = TempDir::new().unwrap();
        let nested = temp.path().join("contest").join("abc");
        fs::create_dir_all(&nested).unwrap();

        fs::write(
            temp.path().join(CONFIG_FILE_NAME),
            "[bundle]\nembed = true\n",
        )
        .unwrap();
        fs::write(
            nested.join(CONFIG_FILE_NAME),
            "[compiler]\npath = \"clang++\"\n",
        )
        .unwrap();

        let source = nested.join("main.cpp");
        fs::write(&source, "int main() {}").unwrap();

        // 近い側のみ採用され、遠い側の embed=true はマージされない。
        let config = resolve(&source).unwrap();
        assert_eq!(config.compiler, PathBuf::from("clang++"));
        assert!(!config.embed);
    }

    #[test]
    fn resolve_reports_parse_error_with_path() {
        let temp = TempDir::new().unwrap();
        fs::write(temp.path().join(CONFIG_FILE_NAME), "this is not = = toml").unwrap();
        let source = temp.path().join("main.cpp");
        fs::write(&source, "int main() {}").unwrap();

        let error = resolve(&source).unwrap_err();
        assert!(error.to_string().contains(CONFIG_FILE_NAME));
    }
}
