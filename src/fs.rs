//! ファイルシステムの横断ユーティリティ。ライブラリのソースツリーを走査し、相対パスを正規化する。
//! ドメインに依存しない低レベル層で、`library` や `bundle` から共有して使う。

pub mod relpath;
pub mod source;
pub mod walk;
