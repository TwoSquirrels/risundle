//! 登録済みライブラリの突き合わせ用インベントリ。`tags.json` を読み込み、バンドルの各工程が必要と
//! する 4 つの問い合わせに答える: インクルードパスの組み立て (`-I`)、`-nostdinc` の要否、ハッシュ
//! 検証、そして `識別子 → 依存ヘッダー` の逆引き。維持指定 (keep) と種別 (`std` / 通常) を保持し、
//! 「維持指定された (Tree-Shaking 対象外の) ライブラリと `std` は識別子情報を使わない」という仕様の
//! 区別を、各メソッドで一貫して適用する。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::fs::relpath;
use crate::library::hash;
use crate::library::local::LocalStore;
use crate::library::tags::{Tags, TagsKind};

/// `std` として扱うライブラリ ID。維持指定時に `-nostdinc` を促す対象。
const STD_ID: &str = "std";

struct Library {
    id: String,
    /// 登録時に保存された絶対パス (realpath 済み)。`-I` と逆引き・分類の基準。
    path: PathBuf,
    /// `$LOCAL/libraries/<id>/dummy`。維持指定時に `-I` で向ける先。
    dummy_dir: PathBuf,
    keep: bool,
    kind: TagsKind,
}

pub struct Inventory {
    libraries: Vec<Library>,
}

impl Inventory {
    /// 登録済みライブラリを全件読み込む。`keep` に含まれる ID は維持指定として印を付ける。
    pub fn load(store: &LocalStore, keep: &BTreeSet<String>) -> Result<Self> {
        let mut libraries = Vec::new();
        for id in store.library_ids()? {
            let tags = Tags::load(&store.tags_json(&id))?;
            libraries.push(Library {
                keep: keep.contains(&id),
                dummy_dir: store.dummy_dir(&id),
                path: tags.path,
                kind: tags.kind,
                id,
            });
        }
        Ok(Self { libraries })
    }

    /// 各ライブラリの `-I` フラグ列。維持指定はダミーへ、それ以外は実パスへ向ける。
    pub fn include_flags(&self) -> Vec<String> {
        self.libraries
            .iter()
            .flat_map(|lib| {
                let dir = if lib.keep { &lib.dummy_dir } else { &lib.path };
                ["-I".to_owned(), dir.to_string_lossy().into_owned()]
            })
            .collect()
    }

    /// `std` が登録済みなら、その認識コンパイラ集合 (正規化済み絶対パス) を返す。未登録なら `None`。
    /// バンドル時に「現在のコンパイラ向けに std が登録されているか」を照合する警告に使う。
    pub fn std_compilers(&self) -> Option<&[PathBuf]> {
        self.libraries
            .iter()
            .find(|lib| lib.id == STD_ID)
            .and_then(|lib| match &lib.kind {
                TagsKind::Std { compilers } => Some(compilers.as_slice()),
                TagsKind::Library { .. } => None,
            })
    }

    /// `std` が登録済みかつ維持指定なら真。`-nostdinc` を付けてダミー経由の解決に倒す合図。
    /// `std` 未登録時に偽を返すことで、ダミーが無い状態で `-nostdinc` を付けて壊すのを防ぐ。
    pub fn uses_nostdinc(&self) -> bool {
        self.libraries
            .iter()
            .any(|lib| lib.id == STD_ID && lib.keep)
    }

    /// 維持指定外かつ `std` 以外のライブラリについて、集約ハッシュを再計算し登録時と比較する。
    /// 維持指定ライブラリと `std` は識別子情報を使わないため検証しない。
    pub fn verify(&self) -> Result<()> {
        for lib in &self.libraries {
            if lib.keep {
                continue;
            }
            let TagsKind::Library { hash: expected, .. } = &lib.kind else {
                continue; // std は検証対象外
            };
            let actual = hash::aggregate(&lib.path).with_context(|| {
                format!("ライブラリ `{}` のハッシュ再計算に失敗しました", lib.id)
            })?;
            if &actual != expected {
                bail!(
                    "ライブラリ `{0}` は登録後に変更されています。`risundle library update {0}` で更新してください",
                    lib.id
                );
            }
        }
        Ok(())
    }

    /// 出力に現れたファイル (`present`) のうち、`<file>` で使われた識別子を 1 つでも定義するものを
    /// 依存ヘッダーとして返す。
    ///
    /// 逆引きの母集合を「linemarker に現れた = 実際に include されたファイル」に限るのが肝。`tags.json`
    /// 全体から無条件に逆引きすると、登録パス配下のテスト用 `.cpp` (`main` 等のありふれた識別子を定義
    /// し、include されない) まで巻き込んでしまう。`present` は維持指定外ライブラリ配下の実在ファイル
    /// (realpath 済み) である前提。
    pub fn dependency_headers(
        &self,
        used: &BTreeSet<String>,
        present: &BTreeSet<PathBuf>,
    ) -> BTreeSet<PathBuf> {
        present
            .iter()
            .filter(|path| {
                self.defined_identifiers(path)
                    .is_some_and(|names| names.iter().any(|name| used.contains(name)))
            })
            .cloned()
            .collect()
    }

    /// realpath 済みパスが属する維持指定外ライブラリを特定し、そのファイルが `tags.json` で定義する
    /// 識別子一覧を返す。linemarker の絶対パスを `files` の相対キーへ (`/` 区切り・`path` prefix 除去で)
    /// 対応づける処理がここに集約される。定義を持たないファイルや対象外パスは `None`。
    fn defined_identifiers(&self, canonical: &Path) -> Option<&[String]> {
        for lib in &self.libraries {
            if lib.keep {
                continue;
            }
            let TagsKind::Library { files, .. } = &lib.kind else {
                continue;
            };
            if let Ok(relative) = canonical.strip_prefix(&lib.path) {
                let key = relpath::to_slash(relative).ok()?;
                return files.get(&key).map(Vec::as_slice);
            }
        }
        None
    }

    /// realpath 済みパスが属するライブラリを `<id>/<ルートからの相対>` (スラッシュ区切り) で表す。
    /// 出力の `#line` にローカル絶対パス (ホームディレクトリ名を含みうる) を残さないための表示用で、
    /// どのライブラリにも属さないパスは `None`。維持指定・`std` も区別せず対象にする (表示の一貫性
    /// のためで、実際に `#line` へ現れるのは展開される非維持ライブラリだけ)。
    pub fn library_relative(&self, canonical: &Path) -> Option<String> {
        for lib in &self.libraries {
            let Ok(relative) = canonical.strip_prefix(&lib.path) else {
                continue;
            };
            let slash = relpath::to_slash(relative).ok()?;
            return Some(format!("{}/{}", lib.id, slash));
        }
        None
    }

    /// 与えた realpath 済みパスが、維持指定外の通常ライブラリ配下なら真。不要ヘッダー判定の候補
    /// (= 出力に現れた、削除しうるヘッダー) を選別するのに使う。`std`・維持指定は削除しない。
    pub fn is_pruneable(&self, canonical: &Path) -> bool {
        self.libraries.iter().any(|lib| {
            !lib.keep
                && matches!(lib.kind, TagsKind::Library { .. })
                && canonical.starts_with(&lib.path)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::BTreeMap;
    use std::fs;

    use tempfile::TempDir;

    /// ソースディレクトリ (canonicalize 可能なよう実在させる) と tags.json を作って登録する。
    fn register(store: &LocalStore, id: &str, kind: TagsKind) -> PathBuf {
        let source = store.library_dir(id).join("source");
        fs::create_dir_all(&source).unwrap();
        let path = source.canonicalize().unwrap();
        Tags {
            path: path.clone(),
            kind,
        }
        .save(&store.tags_json(id))
        .unwrap();
        path
    }

    fn library_kind(files: &[(&str, &[&str])]) -> TagsKind {
        TagsKind::Library {
            hash: "sha256:placeholder".to_owned(),
            files: files
                .iter()
                .map(|(key, names)| {
                    (
                        (*key).to_owned(),
                        names.iter().map(|n| (*n).to_owned()).collect(),
                    )
                })
                .collect(),
        }
    }

    fn keep_set(ids: &[&str]) -> BTreeSet<String> {
        ids.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn include_flags_point_kept_to_dummy_and_others_to_real_path() {
        let local = TempDir::new().unwrap();
        let store = LocalStore::with_root(local.path());
        let std_path = register(
            &store,
            "std",
            TagsKind::Std {
                compilers: vec![std::path::PathBuf::from("/usr/bin/g++")],
            },
        );
        let acl_path = register(&store, "ac-library", library_kind(&[]));

        let inventory = Inventory::load(&store, &keep_set(&["std"])).unwrap();
        let flags = inventory.include_flags();

        // std (維持) はダミーへ、ac-library (非維持) は実パスへ。
        assert!(
            flags
                .windows(2)
                .any(|w| w[0] == "-I" && w[1] == store.dummy_dir("std").to_string_lossy())
        );
        assert!(
            flags
                .windows(2)
                .any(|w| w[0] == "-I" && w[1] == acl_path.to_string_lossy())
        );
        assert!(!flags.iter().any(|f| *f == std_path.to_string_lossy()));
    }

    #[test]
    fn nostdinc_only_when_std_is_kept() {
        let local = TempDir::new().unwrap();
        let store = LocalStore::with_root(local.path());
        register(
            &store,
            "std",
            TagsKind::Std {
                compilers: vec![std::path::PathBuf::from("/usr/bin/g++")],
            },
        );

        assert!(
            Inventory::load(&store, &keep_set(&["std"]))
                .unwrap()
                .uses_nostdinc()
        );
        assert!(
            !Inventory::load(&store, &keep_set(&[]))
                .unwrap()
                .uses_nostdinc()
        );
    }

    #[test]
    fn reverse_lookup_finds_defining_header_among_present_files() {
        let local = TempDir::new().unwrap();
        let store = LocalStore::with_root(local.path());
        let acl_path = register(
            &store,
            "ac-library",
            library_kind(&[("atcoder/dsu.hpp", &["dsu"])]),
        );
        let dsu = acl_path.join("atcoder/dsu.hpp");

        let inventory = Inventory::load(&store, &keep_set(&["std"])).unwrap();
        let headers = inventory.dependency_headers(
            &keep_set(&["dsu", "unrelated"]),
            &BTreeSet::from([dsu.clone()]),
        );

        assert_eq!(headers, BTreeSet::from([dsu]));
    }

    #[test]
    fn files_absent_from_output_are_never_pulled_even_if_they_define_the_identifier() {
        // include されない test/example/*.cpp が `main` を定義していても、present に無ければ依存に
        // ならない (登録パス配下のテスト .cpp 巻き込み回帰)。
        let local = TempDir::new().unwrap();
        let store = LocalStore::with_root(local.path());
        register(
            &store,
            "ac-library",
            library_kind(&[("test/example/dsu_practice.cpp", &["main"])]),
        );

        let inventory = Inventory::load(&store, &keep_set(&["std"])).unwrap();
        // present が空 (= どの linemarker にも現れない) なら、main を使っていても巻き込まれない。
        assert!(
            inventory
                .dependency_headers(&keep_set(&["main"]), &BTreeSet::new())
                .is_empty()
        );
    }

    #[test]
    fn kept_library_is_excluded_from_reverse_lookup() {
        let local = TempDir::new().unwrap();
        let store = LocalStore::with_root(local.path());
        let acl_path = register(
            &store,
            "ac-library",
            library_kind(&[("atcoder/dsu.hpp", &["dsu"])]),
        );
        let dsu = acl_path.join("atcoder/dsu.hpp");

        // ac-library を維持指定すると、その識別子は逆引きされない (Tree-Shaking 対象外)。
        let inventory = Inventory::load(&store, &keep_set(&["ac-library"])).unwrap();
        assert!(
            inventory
                .dependency_headers(&keep_set(&["dsu"]), &BTreeSet::from([dsu]))
                .is_empty()
        );
    }

    #[test]
    fn is_pruneable_only_for_non_kept_library_paths() {
        let local = TempDir::new().unwrap();
        let store = LocalStore::with_root(local.path());
        let std_path = register(
            &store,
            "std",
            TagsKind::Std {
                compilers: vec![std::path::PathBuf::from("/usr/bin/g++")],
            },
        );
        let acl_path = register(&store, "ac-library", library_kind(&[]));

        let inventory = Inventory::load(&store, &keep_set(&["std"])).unwrap();

        assert!(inventory.is_pruneable(&acl_path.join("atcoder/dsu.hpp")));
        assert!(!inventory.is_pruneable(&std_path.join("vector"))); // std は削除しない
        assert!(!inventory.is_pruneable(Path::new("/elsewhere/foo.hpp")));
    }

    #[test]
    fn library_relative_renders_id_based_path_and_none_outside() {
        let local = TempDir::new().unwrap();
        let store = LocalStore::with_root(local.path());
        let acl_path = register(&store, "ac-library", library_kind(&[]));

        let inventory = Inventory::load(&store, &keep_set(&[])).unwrap();

        assert_eq!(
            inventory.library_relative(&acl_path.join("atcoder/dsu.hpp")),
            Some("ac-library/atcoder/dsu.hpp".to_owned())
        );
        assert_eq!(
            inventory.library_relative(Path::new("/elsewhere/foo.hpp")),
            None
        );
    }

    #[test]
    fn verify_passes_for_matching_hash_and_fails_for_mismatch() {
        let local = TempDir::new().unwrap();
        let store = LocalStore::with_root(local.path());

        // 実内容に一致するハッシュを持つライブラリを作る。
        let source = store.library_dir("lib").join("source");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("a.hpp"), "struct a {};").unwrap();
        let path = source.canonicalize().unwrap();
        let real_hash = hash::aggregate(&path).unwrap();

        let mut files = BTreeMap::new();
        files.insert("a.hpp".to_owned(), vec!["a".to_owned()]);
        Tags {
            path,
            kind: TagsKind::Library {
                hash: real_hash,
                files,
            },
        }
        .save(&store.tags_json("lib"))
        .unwrap();

        // 非維持なら検証対象 → 一致して通る。
        assert!(
            Inventory::load(&store, &keep_set(&[]))
                .unwrap()
                .verify()
                .is_ok()
        );

        // 内容を変えるとハッシュ不一致でエラー。
        fs::write(
            store.library_dir("lib").join("source/a.hpp"),
            "struct a { int changed; };",
        )
        .unwrap();
        let error = Inventory::load(&store, &keep_set(&[]))
            .unwrap()
            .verify()
            .unwrap_err();
        assert!(error.to_string().contains("library update"));
    }
}
