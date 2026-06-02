//! 登録済みライブラリのドメイン。`tags.json` のデータモデルと永続化、`$LOCAL` 上のストレージ配置、
//! 登録時の識別子抽出・ダミー生成・集約ハッシュ計算をまとめる。`library` / `bundle` の両コマンドが
//! 共有する中核で、走査の詳細は [`crate::fs`] に委ねる。

pub mod dummy;
pub mod hash;
pub mod identifiers;
pub mod local;
pub mod tags;
