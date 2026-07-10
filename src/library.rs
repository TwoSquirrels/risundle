//! 登録済みライブラリのドメイン。`tags.json` のデータモデルと永続化、`$LOCAL` 上のストレージ配置、
//! 識別子抽出・ダミー生成・集約ハッシュ計算と、それらを束ねる登録処理 (registry) をまとめる。
//! `library` / `bundle` の両コマンドが共有する中核で、走査の詳細は [`crate::fs`] に、コンパイラへの
//! 問い合わせは [`crate::compiler`] に委ねる。

pub mod dummy;
pub mod hash;
pub mod identifiers;
pub mod local;
pub mod registry;
pub mod tags;

#[cfg(test)]
pub mod testutil;
