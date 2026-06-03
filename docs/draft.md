# 競プロ用 C++ バンドラー「risundle」v1.0 仕様案

競技プログラミングにおいて役立つ、Tree-Shaking 機能付きのソースバンドラー「risundle」の v1.0 の仕様案を提示する。

- IWYU と違い、`#include` の削減はせず、バンドル後のファイルサイズを削減することを目指す。
    - v2.0 以降では `#include` 削減も目指していいかも？
- minify もしない。
    - v3.0 以降は minify も目指していいかも？
- 厳密な Tree-Shaking はしない。
    - C++ の厳密な依存検出は難しいので、識別子名の照合による近似的な依存検出を行う。
    - 余分に依存を検出する方向に倒せば、エラーは防げる。
- マクロは展開される。
    - ローカルデバッグ用の `#include` を Tree-Shaking できるようにしたいので。
    - マクロの依存を検出するのが難しく、マクロを残してしまうと Tree-Shaking が難しくなるからという理由もある。
- 複数ソースファイルのバンドルは非対応。
- Rust で開発する。
- 内部保存用のデータは全て Rust でいう [`dirs::data_local_dir()`](https://docs.rs/dirs/latest/dirs/fn.data_local_dir.html) で取得できるディレクトリ配下の `risundle` ディレクトリで管理するものとする。以後このパスを `$LOCAL` と呼ぶ。OS 共有のデータディレクトリ直下を直接使わないのは、他アプリとの衝突を避けるため。

## サブコマンド `library`

- `risundle library add <id> <path>` (ライブラリの登録)
    - `<id>` が `std` の場合はエラー (標準ライブラリは `add-std` で登録する)。
    - `$LOCAL/libraries/<id>/tags.json` が既に存在する場合はエラー。
    - `<path>` をインクルードパスとして、それ以下の全ファイルについて、その中身を例えば `atcoder/modint` なら `#pragma RISUNDLE_DUMMY <atcoder/modint>` にしたファイルで、ディレクトリ構造はそのまま `$LOCAL/libraries/<id>/dummy/` 以下に格納する。
    - `<path>` を `$LOCAL/libraries/<id>/tags.json` に保存。あわせて、[tree-sitter](https://crates.io/crates/tree-sitter) (C++ 文法 [tree-sitter-cpp](https://crates.io/crates/tree-sitter-cpp) と [tree-sitter-tags](https://crates.io/crates/tree-sitter-tags)) で抽出したファイル毎の定義識別子一覧と、`<path>` 以下の内容から計算した集約ハッシュも保存。
- `risundle library add-std [<compiler>]` (標準ライブラリの登録)
    - `<compiler>` (省略時 `g++`) を実体の絶対パスへ正規化し、`<compiler> -E -x c++ -v -` のシステム include 探索パス一覧を検出する (`CPATH` 等の環境変数は除去して汚染を防ぐ)。
    - 既に `std` が登録済みなら、その認識コンパイラ集合へ `<compiler>` を加える (加算式)。集合全コンパイラの探索パスを 1 つの `$LOCAL/libraries/std/dummy/` ツリーへ統合してダミー化する。
    - `tags.json` には代表パス (`path`) と認識コンパイラ集合 (`compilers`、絶対パス) を保存。識別子一覧・集約ハッシュは持たない。
- `risundle library delete <id>` (ライブラリの登録削除)
    - `$LOCAL/libraries/<id>/tags.json` が存在しない場合はエラー。
    - `$LOCAL/libraries/<id>/` を削除。
- `risundle library update [<id> [<path>]]` (ライブラリの更新対応)
    - `<id>` が指定されている場合:
        - `$LOCAL/libraries/<id>/tags.json` が存在しない場合はエラー。
        - `std` の場合は、`tags.json` の認識コンパイラ集合からシステム include を再検出し、統合ツリーを作り直す (`<path>` 指定は不可)。
        - それ以外は、`<path>` 省略時に `$LOCAL/libraries/<id>/tags.json` から参照し、`$LOCAL/libraries/<id>/` を削除して `risundle library add <id> <path>` と同じことをし再生成。
    - `<id>` が指定されていない場合:
        - `$LOCAL/libraries/*/tags.json` をリストアップし、それぞれのディレクトリ名についてそれを `<id>` として「`<id>` が指定されている場合」を実行。
- `risundle library list` (ライブラリ一覧)
    - `$LOCAL/libraries/*/tags.json` をリストアップし、それぞれのディレクトリ名とインクルードパスを出力する。
- `risundle library show [-v | --verbose] <id>`
    - `$LOCAL/libraries/<id>/tags.json` が存在しない場合はエラー。
    - `$LOCAL/libraries/<id>/tags.json` の情報を出力。普段確認したい情報 (ID・インクルードパス・種別・定義識別子を持つファイル数) に絞る。
    - `-v` 指定時は、集約ハッシュと各ファイルの定義識別子一覧も併せて出力する。

## メインコマンド

- `risundle [-c <path> | --compiler=<path>] [-k <id> | --keep=<id>]... [-e | --embed] [-n | --no-check] [--] <file> [<options>]` (バンドル実行)
    - `std` が未登録、または登録済みでも `$compiler` (絶対パスへ正規化) が `std` の認識コンパイラ集合に無い場合、`add-std` を促す警告を出力 (強制はしない)。
    - `<file>` のパスからファイルシステムのルート (`/`) まで親ディレクトリを順に辿り、`.risundlerc.toml` を探す。
        - 最初に見つかったもの (= `<file>` に最も近いもの) をオプションのデフォルト値とする。複数見つかってもマージはしない。
        - 無い場合は、`--compiler=g++ --keep=std` と `-std=gnu++17 -O2 -DONLINE_JUDGE -DATCODER` をデフォルト値とする。
    - `$compiler` を `<path>` に設定。
    - `$options` を `<options>` に設定。
    - `std` が `<id>` に含まれていた場合、`$options` に `-nostdinc` を追記。
    - `$LOCAL/libraries/*/tags.json` を元に、`-I` オプションでインクルードパスを設定するよう `$options` に追記。
        - ただし、`<id>` で維持指定されたライブラリは、ダミーのパスを設定。
    - 維持指定されていない `std` 以外のライブラリそれぞれについて、`path` 以下の集約ハッシュを再計算し、`tags.json` の `hash` と比較する。
        - 維持指定された (Tree-Shaking 対象外の) ライブラリと `std` は識別子情報を使わないため、検証しない。
        - 一致しないライブラリがあれば、ライブラリが更新された旨と `risundle library update <id>` を促すメッセージを出してエラー終了する。`--no-check` 指定時はハッシュ検証自体をスキップする。
    - `$compiler $options -x c++ -E -C <file>` コマンドでプリプロセス結果を取得。
    - [linemarkers](https://gcc.gnu.org/onlinedocs/cpp/Preprocessor-Output.html) を元に `<file>` の部分だけを見て、維持指定されていないライブラリの識別子を検出し、依存ヘッダー一覧を生成。
        - 識別子の検出は [logos](https://crates.io/crates/logos) による字句解析で完全一致 (単語境界) を使用。文字列リテラル・コメントはスキップして誤検出を抑える。C++ raw string リテラル (`R"..."`) 内の誤検出は許容する。
            - 検出は字句レベルで足りるため、列挙側の tree-sitter ではなく logos を使う。linemarker 混じりのプリプロセス後テキストには構文解析より字句解析が頑健で、検出側で最優先の「取りこぼし (= 依存漏れ = コンパイルエラー) を避ける」に適うため (過剰検出は無害)。
        - 検出した識別子から、各ライブラリの `tags.json` の `files` を逆引きして依存ヘッダーを特定する。逆引きインデックスはバンドル時にメモリ上で構築する (`tags.json` には持たない)。
        - linemarker の絶対パスから `files` の相対キーを引く際は、双方を `realpath` で正規化し、パス区切りは `/` に統一した上で、`path` を prefix として除去して照合する。
    - 全ての依存ヘッダーに対して、`$compiler $options -x c++ -M` を実行して、維持指定されていないライブラリのヘッダーのうちこれに一度も含まれていなかったヘッダーで不要ヘッダー一覧を生成。
    - プリプロセス結果を上からスキャンし、以下のルールで置換していく。
        - `#pragma RISUNDLE_DUMMY` が含まれるヘッダーは、`#include` に置換。
            - 復元する `#include` は山括弧 (`<...>`) 形式に固定する。ダミーが表すのは必ず `-I` で解決される維持ライブラリであり、プリプロセス後の pragma 行からは元が `<>` か `""` か区別できない (情報が失われる) ため。引用符で書かれていても山括弧へ正規化され、`-I` 経由で同じく解決される。
            - **既知の制限**: 維持ライブラリを `#include "..."` で書き、かつバンドル対象ファイルと同じディレクトリに同名パス (例: `./atcoder/dsu`) が偶然存在すると、`""` のカレント探索が優先されてダミーがバイパスされ、維持指定したライブラリが展開されてしまう。`""` のソースディレクトリ探索は C++ 標準動作でコンパイラから抑制できないため、`<>` での include を推奨する。将来的には、linemarker が指すパスが維持ライブラリの `path` 配下でなくバイパスされた形跡を検出して警告する余地がある (バンドル実装時に linemarker 処理と併せて判断)。
        - 不要ヘッダー一覧に含まれるヘッダーは、そのヘッダー自身のコード行のみ削除。そのスコープ内にネストされた別ヘッダーのスコープは、そのヘッダーの要否に基づいて独立して判断する。
    - linemarker は、生き残るコードの直前のものだけ出力に残す。(バンドル後のコンパイラ診断の行番号が元ファイルを指すようにするため。) コードが丸ごと削除されたヘッダーの linemarker は、指し示す先が無くなるためコードと一緒に削除する。
        - 残す marker は GCC の linemarker (`# <行> "<ファイル>" <フラグ>`) ではなく、標準の `#line <行> "<ファイル>"` ディレクティブとして出力する。バンドル結果は提出される「ソース」であり、`# <行> ...` はプリプロセッサ「出力」用の GNU 拡張なので、規格準拠で可搬な `#line` がふさわしい。入れ子フラグ (1=進入, 2=復帰 等) は平坦化後に意味を持たず、残すと "linemarker ignored due to incorrect nesting" 警告の元になるため落とす (`#line` はフラグを持たない)。
    - `--embed` オプションがある場合、先頭に `// ` コメントで `<file>` のオリジナルコードを添付。
    - 先頭に `// Bundled with risundle v1.0` のような簡易的なクレジット表記を追加。
    - 完成したコードを出力。

## グローバルオプション

- `risundle (-h | --help)`
    - コマンドに関わらず、ヘルプメッセージを出力。
- `risundle (-v | --version)`
    - コマンドに関わらず、バージョン情報を出力。

## `tags.json` フォーマット

`library add` が生成し、バンドルコマンドが読み込む中核データ構造。マシンローカルなキャッシュであり、可搬性は持たない (`path` は絶対パス)。

`<id>` が `std` でない場合:

```json
{
  "schema_version": 1,
  "path": "/usr/local/include",
  "hash": "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
  "files": {
    "atcoder/modint.hpp": ["modint", "modint_base", "modint_common"],
    "atcoder/segtree.hpp": ["segtree"]
  }
}
```

`<id>` が `std` の場合 (識別子一覧を持たず、更新検知の対象外のため `files`・`hash` を省略。代わりに、システム include パスの自動検出に用いた `compilers` (正規化済み絶対パスの集合) を持つ):

```json
{
  "schema_version": 1,
  "path": "/usr/include/c++/12",
  "compilers": ["/usr/bin/x86_64-linux-gnu-g++-14"]
}
```

- `schema_version`: スキーマの互換性チェック用の整数。未知の値の場合は再生成を促すエラーを出す。
- `path`: `library add` で指定されたインクルードパス (絶対パス)。`-I` オプションへそのまま渡す。`std` では検出したシステム include パスのうち代表 (最初のコンパイラの C++ 標準ライブラリ dir) を記録する (表示用)。
- `compilers`: `std` のみ。`add-std` で std を認識させたコンパイラの集合 (実体の絶対パスへ正規化済み)。`update` 時の再検出と、バンドル時の「現在のコンパイラ向けに std が登録されているか」の照合警告に使う。
- `hash`: `path` 以下の全ファイルの相対パスと内容から計算した集約ハッシュ (`sha256:` プレフィックス付き)。ライブラリの更新検知に使う。mtime ではなく内容ベースなので `git clone` や `cp` での時刻変化に影響されず、相対パスも含めるためファイルの追加・削除・リネームも検知できる。
- `files`: ライブラリルート (`path`) からの相対パスをキーとし、そのファイルが定義する識別子名の配列を値とする。tree-sitter-cpp の tags クエリで各ファイルの定義シンボルを取得し、kind は持たず名前のみ。
    - `std` は `files`・`hash` を省略し `compilers` を持つ。`std` 以外は `files`・`hash` の両方を必ず持つ (`files` は識別子が一つも無くても空オブジェクト `{}`)。これにより「`std`」と「識別子 0 件のライブラリ」を構造で区別する。

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

インストール時、自動的に `risundle library add-std` 相当の動作をする。`add-std` は引数で渡されたコンパイラ (省略時 `g++`) のシステム include 探索パスを自動検出し、C++ 標準ライブラリ・コンパイラ組み込み (`immintrin.h` 等)・アーキ依存 (`bits/stdc++.h` 等)・C ライブラリの全 dir をまとめてダミー化する。検出に失敗した場合は警告。

`add-std` は「単一の現在コンパイラ」をグローバルに固定するのではなく、**認識しているコンパイラの集合**を育てる。`add-std clang++` のように繰り返すと、その都度コンパイラを集合に加え、集合全コンパイラのシステム include を 1 つのダミーツリーへ統合する (ダミーは pragma 1 行で、復元は同名 `#include` になるため混在しても無害)。コンパイラは実体の絶対パスへ正規化して `g++` と `/usr/bin/g++` の表記揺れを防ぐ。バンドル時、対象コンパイラが認識集合に無ければ `add-std <それ>` を促す警告を出す (フェイルファスト)。`update std` は記録済み集合から統合ツリーを再構築する。

### バージョンアップ

```sh
cargo install-update risundle
```

### アンインストール

```sh
cargo uninstall risundle
```
