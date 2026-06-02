//! Tree-Shaking の中核ロジック。プリプロセス出力の解析 (linemarker による行の出所判定、
//! 後続で識別子検出) を担い、登録済みライブラリ情報 ([`crate::library`]) を逆引きに使う。
//! 外部コンパイラの起動とコマンド配線は [`crate::commands::bundle`] が持つ。

pub mod detect;
pub mod inventory;
pub mod linemarker;
pub mod prune;
pub mod rewrite;
