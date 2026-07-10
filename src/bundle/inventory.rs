//! 登録済みライブラリの突き合わせ用インベントリ。`tags.json` を読み込み、バンドルの各工程が必要と
//! する問い合わせに答える: インクルードパスの組み立て (`-I`)、`-nostdinc` の要否、ハッシュ検証、
//! `識別子 → 依存ヘッダー` の逆引き、そして `定義した型 → その実装ファイル` の逆引き。維持指定
//! (keep) と種別 (`std` / 通常) を保持し、
//! 「維持指定された (tree-shaking 対象外の) ライブラリと `std` は識別子情報を使わない」という仕様の
//! 区別を、各メソッドで一貫して適用する。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::fs::relpath;
use crate::library::hash;
use crate::library::local::LocalStore;
use crate::library::registry::STD_ID;
use crate::library::tags::{Tags, TagsKind};

struct Library {
    id: String,
    /// 登録時に保存された絶対パス (realpath 済み)。`-I` と逆引き・分類の基準。
    path: PathBuf,
    /// `$LOCAL/libraries/<id>/dummy`。維持指定時に `-I` で向ける先。
    dummy_dir: PathBuf,
    keep: bool,
    kind: TagsKind,
}

/// 1 ファイルの `tags.json` レコード。定義識別子と実装先の型名を対で保持する。
struct FileTags<'a> {
    defines: &'a [String],
    implements: &'a [String],
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
            let actual = hash::aggregate(&lib.path)
                .with_context(|| format!("failed to recompute the hash of library `{}`", lib.id))?;
            if &actual != expected {
                bail!(
                    "library `{0}` has changed since registration; run `risundle library update {0}` to update it",
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
                self.file_tags(path)
                    .is_some_and(|tags| tags.defines.iter().any(|name| used.contains(name)))
            })
            .cloned()
            .collect()
    }

    /// `present` のうち、`needed` 内のいずれかのファイルが定義する型を実装しているファイルを返す。
    ///
    /// 「実装している」は登録時に記録した実装先の型名 (`tags.json` の `implements`) と、`needed` 側の
    /// 定義識別子との照合で判定する。演算子オーバーロードのような、定義識別子として現れない依存を
    /// 拾うための逆引き。
    pub fn implementation_files(
        &self,
        needed: &BTreeSet<PathBuf>,
        present: &BTreeSet<PathBuf>,
    ) -> BTreeSet<PathBuf> {
        let needed_names: BTreeSet<&String> = needed
            .iter()
            .filter_map(|path| self.file_tags(path))
            .flat_map(|tags| tags.defines)
            .collect();
        present
            .iter()
            .filter(|path| {
                self.file_tags(path)
                    .is_some_and(|tags| tags.implements.iter().any(|t| needed_names.contains(t)))
            })
            .cloned()
            .collect()
    }

    /// realpath 済みパスが維持指定外ライブラリ配下なら、そのファイルの `tags.json` レコードを返す。
    /// 対象外パスは `None`。ライブラリ配下だが定義や実装先を持たないファイルは、対応するスライスが
    /// 空になる。linemarker の絶対パスを相対キーへ (`/` 区切り・`path` prefix 除去で) 対応づける処理が
    /// ここに集約される。
    fn file_tags(&self, canonical: &Path) -> Option<FileTags<'_>> {
        for lib in &self.libraries {
            if lib.keep {
                continue;
            }
            let TagsKind::Library {
                files, implements, ..
            } = &lib.kind
            else {
                continue;
            };
            if let Ok(relative) = canonical.strip_prefix(&lib.path) {
                let key = relpath::to_slash(relative).ok()?;
                return Some(FileTags {
                    defines: files.get(&key).map(Vec::as_slice).unwrap_or_default(),
                    implements: implements.get(&key).map(Vec::as_slice).unwrap_or_default(),
                });
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
        library_kind_with_implements(files, &[])
    }

    fn library_kind_with_implements(
        files: &[(&str, &[&str])],
        implements: &[(&str, &[&str])],
    ) -> TagsKind {
        let to_map = |entries: &[(&str, &[&str])]| {
            entries
                .iter()
                .map(|(key, names)| {
                    (
                        (*key).to_owned(),
                        names.iter().map(|n| (*n).to_owned()).collect(),
                    )
                })
                .collect()
        };
        TagsKind::Library {
            hash: "sha256:placeholder".to_owned(),
            files: to_map(files),
            implements: to_map(implements),
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
    fn implementation_files_are_found_via_needed_definitions() {
        let local = TempDir::new().unwrap();
        let store = LocalStore::with_root(local.path());
        let lib_path = register(
            &store,
            "mylib",
            library_kind_with_implements(
                &[("fps.hpp", &["FPS"]), ("other.hpp", &["other"])],
                &[
                    ("fps-impl.hpp", &["FPS"]),
                    ("unrelated-impl.hpp", &["Tree"]),
                ],
            ),
        );
        let fps = lib_path.join("fps.hpp");
        let implementation = lib_path.join("fps-impl.hpp");
        let present = BTreeSet::from([
            fps.clone(),
            implementation.clone(),
            lib_path.join("other.hpp"),
            lib_path.join("unrelated-impl.hpp"),
        ]);

        let inventory = Inventory::load(&store, &keep_set(&["std"])).unwrap();
        let files = inventory.implementation_files(&BTreeSet::from([fps]), &present);

        // needed が定義する FPS の実装ファイルだけが選ばれ、別の型 (Tree) の実装は巻き込まない。
        assert_eq!(files, BTreeSet::from([implementation]));
    }

    #[test]
    fn implementation_files_outside_present_are_not_pulled() {
        let local = TempDir::new().unwrap();
        let store = LocalStore::with_root(local.path());
        let lib_path = register(
            &store,
            "mylib",
            library_kind_with_implements(&[("fps.hpp", &["FPS"])], &[("fps-impl.hpp", &["FPS"])]),
        );
        let fps = lib_path.join("fps.hpp");

        let inventory = Inventory::load(&store, &keep_set(&["std"])).unwrap();
        // present に無い (= include されていない) ファイルは、実装先が一致しても補わない。
        assert!(
            inventory
                .implementation_files(&BTreeSet::from([fps.clone()]), &BTreeSet::from([fps]))
                .is_empty()
        );
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

        // ac-library を維持指定すると、その識別子は逆引きされない (tree-shaking 対象外)。
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
                implements: BTreeMap::new(),
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
