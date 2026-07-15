# risundle

**tree-shaking 機能付き、競技プログラミング用 C++ ソースバンドラー**

[English](README.md) | 日本語

[![CI](https://github.com/TwoSquirrels/risundle/actions/workflows/ci.yml/badge.svg)](https://github.com/TwoSquirrels/risundle/actions/workflows/ci.yml)
[![coverage](https://img.shields.io/endpoint?url=https://twosquirrels.github.io/risundle/badge.json)](https://twosquirrels.github.io/risundle/)
[![crates.io](https://img.shields.io/crates/v/risundle.svg)](https://crates.io/crates/risundle)
[![license](https://img.shields.io/crates/l/risundle.svg)](LICENSE)

競技プログラミングの解答を、ライブラリ込みで提出用の 1 ファイルにまとめます。include をそのまま展開する [oj-bundle](https://github.com/online-judge-tools/verification-helper) などと違い、解答が実際に使っているヘッダーファイルだけを残す tree-shaking を行うため、バンドル後のファイルが小さくなります。

## 特徴

- IWYU のような重い静的解析を必要とせず、手元のコンパイラのプリプロセスだけで完結する、競技プログラミング提出に特化したツールです。
- 解答が実際に使っているヘッダーファイルだけを残すので、提出サイズ制限の厳しいジャッジでも通りやすくなります。
- 自作ライブラリを全部 include したテンプレートを 1 つ用意すれば、問題ごとに include を切り替える必要がありません。

> [!NOTE]
> risundle が正しくバンドルできるのは、ある程度行儀の良いファイル分割をしているライブラリです。宣言と実装が別ファイルのライブラリにも対応していますが、演算子オーバーロードだけを書いたファイルなどでは定義が消え、コンパイルエラーやリンクエラーになる可能性があります。詳しい条件は [docs/compatibility.ja.md](docs/compatibility.ja.md) を参照してください。

## バンドル例

たとえば、`modint.hpp` を土台にして `modpow.hpp` と `combination.hpp` を重ねた、3 ファイルの自作ライブラリを `mylib` という ID で登録しておきます。

```cpp
// modint.hpp
#pragma once

constexpr long long MOD = 998244353;

struct ModInt {
    long long v = 0;
    ModInt(long long v) : v(v % MOD) {}
    ModInt& operator*=(ModInt o) { v = v * o.v % MOD; return *this; }
    // ほか 100 行程度の実装
};

// modpow.hpp
#pragma once
#include "modint.hpp"

ModInt modpow(ModInt a, long long n) {
    ModInt r = 1;
    for (; n > 0; n >>= 1, a *= a)
        if (n & 1) r *= a;
    return r;
}

// combination.hpp
#pragma once
#include "modint.hpp"

struct Combination {
    // 100 行程度の実装
};
```

解答はこのライブラリから 2 ファイルを include していますが、実際に使うのは `modpow` だけです。

```cpp
// main.cpp
#include <bits/stdc++.h>
#include <modpow.hpp>
#include <combination.hpp>

int main() {
    std::cout << modpow(2, 100).v << std::endl;
}
```

これをバンドルすると、次の 1 ファイルにまとまります。

```cpp
// submission.cpp
// Bundled with risundle v2.0.0
#line 1 "main.cpp"
#include <bits/stdc++.h>
#line 1 "mylib/modpow.hpp"
       
#line 1 "mylib/modint.hpp"
       

constexpr long long MOD = 998244353;

struct ModInt {
    long long v = 0;
    ModInt(long long v) : v(v % MOD) {}
    ModInt& operator*=(ModInt o) { v = v * o.v % MOD; return *this; }
    // ほか 100 行程度の実装
};
#line 3 "mylib/modpow.hpp"

ModInt modpow(ModInt a, long long n) {
    ModInt r = 1;
    for (; n > 0; n >>= 1, a *= a)
        if (n & 1) r *= a;
    return r;
}
#line 4 "main.cpp"

int main() {
    std::cout << modpow(2, 100).v << std::endl;
}
```

include していても使っていない `combination.hpp` は削除され、`modpow.hpp` が依存する `modint.hpp` は維持されます。標準ライブラリは `#include` のまま残ります。`#line` はコンパイラの診断を元ファイルの行番号で表示させるためのもので、ジャッジのエラーも元ファイル基準で読めます。

## インストール

[Rust ツールチェーン](https://www.rust-lang.org/tools/install) と、GCC 互換の C++ コンパイラ (`g++` や `clang++` など) が必要です。risundle が依存する `-E`/`-M`/`-v` オプションを持たない MSVC には対応していません。

```bash
cargo install risundle
```

更新には [cargo-update](https://crates.io/crates/cargo-update) が使えます。

```bash
cargo install-update risundle
```

`risundle library` サブコマンド実行時、新しいバージョンが出ていれば標準エラーに一言案内が出ます (`risundle` 本体、つまりバンドル実行時には出ません)。`RISUNDLE_NO_UPDATE_CHECK` を設定すると無効化できます。

## クイックスタート

```bash
# 自作ライブラリを登録する (ID は任意)
risundle library add mylib ~/cp/library

# 登録したライブラリを include した解答を、1 ファイルへバンドルする
risundle main.cpp > submission.cpp
```

`std` は初回バンドル時に自動登録され、既定で温存されます。

## 使い方

「〜したい」から引く逆引き集は [docs/cheatsheet.ja.md](docs/cheatsheet.ja.md) にあります。以下は機能ごとの説明です。

### バンドル

```
risundle [OPTIONS] <FILE> [-- <COMPILER OPTIONS>...]
```

`<FILE>` をバンドルし、結果を標準出力へ書き出します。

| オプション | 説明 |
| --- | --- |
| `-c`, `--compiler <PATH>` | 使用するコンパイラ (既定: `g++`) |
| `-k`, `--keep <ID>` | 展開せず `#include` のまま残すライブラリを追加指定する (繰り返し可。既定: `std`) |
| `--no-keep <ID>` | 維持指定 (keep) からライブラリを外す (繰り返し可。`--keep` より優先) |
| `--no-tree-shaking` | tree-shaking を無効化し、keep 指定以外をすべて展開する (フォールバック用) |
| `-e`, `--embed` | 元のソースを先頭にコメントとして埋め込む |
| `--no-embed` | 元のソースを埋め込まない (設定の `embed = true` を打ち消す) |
| `-n`, `--no-check` | ライブラリ更新のハッシュ検証をスキップする |
| `--no-config` | `.risundlerc.toml` を無視する (設定ファイルが無い時と同じ挙動) |
| `-- <OPTIONS>...` | `--` 以降をコンパイラへそのまま渡す (設定の options への追記) |

`--keep` はライブラリを展開せず `#include` のまま残しますが、`--no-tree-shaking` は keep 指定を除く全ライブラリを展開した上で tree-shaking を行いません。両者は別物で、併用もできます。なお `--no-tree-shaking` は識別子情報を使わないため、ライブラリ更新のハッシュ検証も行いません。

```bash
# clang++ を使い、AC Library も展開せず #include のまま残す (std は既定で維持される)
risundle -c clang++ -k ac-library main.cpp > submission.cpp

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

解答ファイルのあるディレクトリから親方向に探索し、最も近い `.risundlerc.toml` を 1 つ採用します (複数ファイルのマージはしません)。CLI で明示したオプションが設定ファイルより優先されます: スカラーと bool は上書き、維持指定 (keep) は `--keep` で追加・`--no-keep` で除外、`--` のコンパイラオプションは設定への追記です。`--no-config` を指定すると設定ファイルを無視できます。

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

IWYU (include-what-you-use 0.21) と実行時間を比較しました。環境は WSL 2 (Ubuntu 24.04、Intel Core 7 240H、g++ 14.2) です。

| ライブラリ | risundle | IWYU |
| --- | --- | --- |
| AC Library | 0.031 秒 | 0.491 秒 |
| Nyaan's Library | 0.033 秒 | 2.085 秒 |

risundle はライブラリ規模によらずほぼ一定で、IWYU はヘッダー数が増えるほど伸びます。IWYU は clang の AST をフル構築するのに対し、risundle はコンパイラのプリプロセス (`-E`/`-M`) だけで完結するためです。なお IWYU と risundle は目的が異なり (IWYU は `#include` の修正提案、risundle はバンドル)、同じ問題を解くツールではありません。

## 仕組み

1. プリプロセス (`-E`) で include を展開する。維持指定 (`keep`) のライブラリはダミー経由で `#include` のまま残す。
2. 解答が使う識別子を字句解析で検出し、登録済みライブラリの定義から依存ヘッダーを逆引きする。
3. `-M` で必要なヘッダーの推移閉包を求め (必要になった型の実装ファイルも維持する)、出力に残った不要なヘッダーを削除する。
4. `#line` ディレクティブで元の出所を保ちつつ、1 ファイルへ再構成する。

include の展開はコンパイラに任せているため、`#pragma once` も手動インクルードガードも正しく扱われます。各コマンドの挙動やエラー条件の詳細は [docs/spec.ja.md](docs/spec.ja.md) を参照してください。

## 開発

機能仕様は [docs/spec.ja.md](docs/spec.ja.md)、対応できるライブラリの条件は [docs/compatibility.ja.md](docs/compatibility.ja.md)、内部設計の方針は [docs/architecture.md](docs/architecture.md) にまとめています。

## ライセンス

[MIT License](LICENSE) — © 2026 TwoSquirrels
