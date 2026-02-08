# 競プロ用 C++ バンドラー「risundle」v1.0 仕様案

競技プログラミングにおいて役立つ、Tree-Shaking 機能付きのソースバンドラー「risundle」の v1.0 の仕様案を提示する。

- IWYU と違い、`#include` の削減はせず、バンドル後のファイルサイズを削減することを目指す。
    - v2.0 以降では `#include` 削減も目指していいかも？
- minify もしない。
    - v3.0 以降は minify も目指していいかも？
- 厳密な Tree-Shaking はしない。
    - C++ の依存検出は難しいので、Ctags によるキーワード検出で依存検出を行う。
    - 余分に依存を検出する方向に倒せば、エラーは防げる。
- マクロは展開される。
    - ローカルデバッグ用の `#include` を Tree-Shaking できるようにしたいので。
    - マクロの依存を検出するのが難しく、マクロを残してしまうと Tree-Shaking が難しくなるからという理由もある。
- 複数ソースファイルのバンドルは非対応。
- Rust で開発する。
- 内部保存用のデータは全て Rust でいう [`dirs::data_local_dir()`](https://docs.rs/dirs/latest/dirs/fn.data_local_dir.html) で取得できるディレクトリで管理するものとする。以後このパスを `$LOCAL` と呼ぶ。

## サブコマンド `library`

- `risundle library add <id> <path>` (ライブラリの登録)
    - `$LOCAL/libraries/<id>/tags.json` が既に存在する場合はエラー。
    - `<path>` をインクルードパスとして、それ以下の全ファイルについて、その中身を例えば `atcoder/modint` なら `#pragma RISUNDLE_DUMMY <atcoder/modint>` にしたファイルで、ディレクトリ構造はそのまま `$LOCAL/libraries/<id>/dummy/` 以下に格納する。
    - `<path>` をインクルードパスとして、`<id>` が `std` でない場合は [ptags (Universal Ctags ラッパー)](https://crates.io/crates/ptags) によってファイル毎の識別子一覧を取得し、`<path>` や現在時刻と共に `$LOCAL/libraries/<id>/tags.json` に保存。
- `risundle library delete <id>` (ライブラリの登録削除)
    - `$LOCAL/libraries/<id>/tags.json` が存在しない場合はエラー。
    - `$LOCAL/libraries/<id>/` を削除。
- `risundle library update [<id> [<path>]]` (ライブラリの更新対応)
    - `<id>` が指定されている場合:
        - `$LOCAL/libraries/<id>/tags.json` が存在しない場合はエラー。
        - `<path>` が指定されていない場合は `$LOCAL/libraries/<id>/tags.json` から参照。
        -  `$LOCAL/libraries/<id>/` を削除。
        - `risundle library add <id> <path>` と同じことをし、再生成。
    - `<id>` が指定されていない場合:
        - `$LOCAL/libaries/*/tags.json` をリストアップし、それぞれのディレクトリ名についてそれを `<id>` として「`<id>` が指定されている場合」を実行。
- `risundle library list` (ライブラリ一覧)
    - `$LOCAL/libraries/*/tags.json` をリストアップし、それぞれのディレクトリ名とインクルードパスを出力する。
- `risundle library show <id>`
    - `$LOCAL/libraries/<id>/tags.json` が存在しない場合はエラー。
    - `$LOCAL/libraries/<id>/tags.json` の情報と、`$LOCAL/libraries/<id>/dummy/` 以下のファイル一覧を出力。

## メインコマンド

- `risundle [-c <path> | --compiler=<path>] [-k <id> | --keep=<id>]... [-e | --embed] [--] <file> [<options>]` (バンドル実行)
    - `$LOCAL/libraries/std/tags.json` が存在しない場合、警告を出力。
    - `<file>` のパスからルートディレクトリまで順に、`.risundlerc.toml` を探す。
        - もしあればそれをオプションのデフォルト値とする。
        - 無い場合は、`--compiler=g++ --keep=std` と `-std=gnu++17 -O2 -DONLINE_JUDGE -DATCODER` をデフォルト値とする。
    - `$compiler` を `<path>` に設定。
    - `$options` を `<options>` に設定。
    - `std` が `<id>` に含まれていた場合、`$options` に `-nostdinc` を追記。
    - `$LOCAL/libraries/*/tags.json` を元に、`-I` オプションでインクルードパスを設定するよう `$options` に追記。
        - ただし、`<id>` で維持指定されたライブラリは、ダミーのパスを設定。
    - `$compiler $options -x c++ -E -C <file>` コマンドでプリプロセス結果を取得。
    - [linemarkers](https://gcc.gnu.org/onlinedocs/cpp/Preprocessor-Output.html) を元に `<file>` の部分だけを見て、維持指定されていないライブラリの識別子を検出し、依存ヘッダー一覧を生成。
    - 全ての依存ヘッダーに対して、`$compiler $options -x c++ -M` を実行して、維持指定されていないライブラリのヘッダーのうちこれに一度も含まれていなかったヘッダーで不要ヘッダー一覧を生成。
    - プリプロセス結果を上からスキャンし、以下のルールで置換していく。
        - `#pragma RISUNDLE_DUMMY` が含まれるヘッダーは、`#include` に置換。
        - 不要ヘッダー一覧に含まれるヘッダーは、そのヘッダーのその階層だけ削除。(挟まれている別のヘッダーを削除することはしない)
    - `--embed` オプションがある場合、先頭に `// ` コメントで `<file>` のオリジナルコードを添付。
    - 先頭に `// Bundled with risundle v1.0` のような簡易的なクレジット表記を追加。
    - 完成したコードを出力。

## グローバルオプション

- `risundle (-h | --help)`
    - コマンドに関わらず、ヘルプメッセージを出力。
- `risundle (-v | --version)`
    - コマンドに関わらず、バージョン情報を出力。

## `.risundlerc.toml` フォーマット

```toml
[compiler]
path = "/usr/bin/clang++"
options = ["-std=gnu++2b", "-O2", "-DONLINE_JUDGE", "-DATCODER"]

[library]
keep = ["std", "ac-library"]

# v1.1 以降実装予定
# [library.paths]
# my-lib = "./library"

[bundle]
embed = true
```

## インストール

```sh
rustup update
cargo install cargo-update risundle
```

インストール時、自動的に C++ の標準ライブラリを探し、`risundle library add std <path>` 相当の動作をする。見つからなかった場合は警告。

### バージョンアップ

```sh
cargo install-update risundle
```

### アンインストール

```sh
cargo uninstall risundle
```
