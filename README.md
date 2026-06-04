# risundle

**Tree-Shaking 機能付き、競技プログラミング用 C++ ソースバンドラー**

[![crates.io](https://img.shields.io/crates/v/risundle.svg)](https://crates.io/crates/risundle)
[![license](https://img.shields.io/crates/l/risundle.svg)](LICENSE)

競技プログラミングの解答を、ライブラリ込みで提出用の 1 ファイルにまとめます。include をそのまま展開する [oj-bundle](https://github.com/online-judge-tools/verification-helper) などと違い、解答が実際に使っている部分だけを残す Tree-Shaking を行うため、バンドル後のファイルが小さくなります。

## 特徴

- IWYU のような重い静的解析を必要とせず、手元のコンパイラのプリプロセスだけで完結する、競技プログラミング提出に特化したツールです。
- 解答が実際に使っているコードだけを残すので、提出サイズ制限の厳しいジャッジでも通りやすくなります。
- 自作ライブラリを全部 include したテンプレートを 1 つ用意すれば、問題ごとに include を切り替える必要がありません。

> [!WARNING]
> v1.0 では、ヘッダーオンリーライブラリのみをサポートしています。宣言と実装が別のファイルに分かれているライブラリは、バンドル後に定義が消えてコンパイルエラーになる可能性があります。

## インストール

[Rust ツールチェーン](https://www.rust-lang.org/tools/install) と C++ コンパイラ (`g++` など) が必要です。

```bash
cargo install risundle
```

更新には [cargo-update](https://crates.io/crates/cargo-update) が使えます。

```bash
cargo install-update risundle
```

## クイックスタート

```bash
# 自作ライブラリを登録する (ID は任意)
risundle library add mylib ~/cp/library

# 登録したライブラリを include した解答を、1 ファイルへバンドルする
risundle main.cpp > submission.cpp
```

`std` は初回バンドル時に自動登録され、既定で温存されます。

## 使い方

### バンドル

```
risundle [OPTIONS] <FILE> [-- <COMPILER OPTIONS>...]
```

`<FILE>` をバンドルし、結果を標準出力へ書き出します。

| オプション | 説明 |
| --- | --- |
| `-c`, `--compiler <PATH>` | 使用するコンパイラ (既定: `g++`) |
| `-k`, `--keep <ID>` | Tree-Shaking の対象外にするライブラリ ID (繰り返し可。既定: `std`) |
| `-e`, `--embed` | 元のソースを先頭にコメントとして埋め込む |
| `-n`, `--no-check` | ライブラリ更新のハッシュ検証をスキップする |
| `-- <OPTIONS>...` | `--` 以降をコンパイラへそのまま渡す |

```bash
# clang++ を使い、AC Library も展開せず #include のまま残す
risundle -c clang++ -k std -k ac-library main.cpp > submission.cpp

# コンパイラに追加オプションを渡す
risundle main.cpp -- -std=gnu++20 -O2
```

### ライブラリ管理

```
risundle library <SUBCOMMAND>
```

| サブコマンド | 説明 |
| --- | --- |
| `add <ID> <PATH>` | ライブラリを登録する |
| `add-std [COMPILER]` | 標準ライブラリ (`std`) を登録する (既定: `g++`) |
| `list` | 登録済みライブラリを一覧する |
| `show <ID> [-v]` | ライブラリの詳細を表示する |
| `update [ID] [PATH]` | ライブラリの変更を反映する (ID 省略時は全ライブラリ) |
| `delete <ID>` | ライブラリの登録を削除する |

`add-std` は複数回呼べます。`risundle library add-std clang++` のようにコンパイラを足すと、それぞれのシステム include を統合し、使い分けられます。

## 設定ファイル

解答ファイルのあるディレクトリから親方向に探索し、最も近い `.risundlerc.toml` を 1 つ採用します (複数ファイルのマージはしません)。CLI オプションが設定ファイルより優先されます。

```toml
[compiler]
path = "g++"
options = ["-std=gnu++17", "-O2", "-DONLINE_JUDGE", "-DATCODER"]

[library]
keep = ["std"]

[bundle]
embed = false
```

上記は既定値です。省略した項目はこの既定値で補われます。

## ベンチマーク

IWYU (include-what-you-use 0.21) と実行時間を比較しました。環境は WSL2 (Ubuntu 24.04、Intel Core 7 240H、g++ 14.2) です。

| ライブラリ | risundle | IWYU |
| --- | --- | --- |
| AC Library | 0.031 秒 | 0.491 秒 |
| Nyaan Library | 0.033 秒 | 2.085 秒 |

risundle はライブラリ規模によらずほぼ一定で、IWYU はヘッダー数が増えるほど伸びます。IWYU は clang の AST をフル構築するのに対し、risundle はコンパイラのプリプロセス (`-E`/`-M`) だけで完結するためです。なお IWYU と risundle は目的が異なり (IWYU は `#include` の修正提案、risundle はバンドル)、同じ問題を解くツールではありません。

## 仕組み

1. プリプロセス (`-E`) で include を展開する。維持指定 (`keep`) のライブラリはダミー経由で `#include` のまま残す。
2. 解答が使う識別子を字句解析で検出し、登録済みライブラリの定義から依存ヘッダーを逆引きする。
3. `-M` で必要なヘッダーの推移閉包を求め、出力に残った不要なヘッダーを削除する。
4. `#line` ディレクティブで元の出所を保ちつつ、1 ファイルへ再構成する。

include の展開はコンパイラに任せているため、`#pragma once` も手動インクルードガードも正しく扱われます。

## 開発

機能仕様は [docs/spec.md](docs/spec.md)、内部設計の方針は [docs/architecture.md](docs/architecture.md) にまとめています。

## ライセンス

[MIT License](LICENSE) — © 2026 TwoSquirrels
