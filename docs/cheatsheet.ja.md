# risundle チートシート

[English](cheatsheet.md) | 日本語

「〜したい」から引く逆引き集です。コマンドはそのままコピペで動きます。各機能の説明は [README](../README.ja.md)、細かい仕様は [spec.ja.md](spec.ja.md) へ。

- [最初に 1 回だけやること](#最初に-1-回だけやること)
- [提出するたびにやること](#提出するたびにやること)
- [提出ファイルを調整したい](#提出ファイルを調整したい)
- [ライブラリを触ったらやること](#ライブラリを触ったらやること)
- [毎回同じオプションを打ちたくない](#毎回同じオプションを打ちたくない)
- [うまくいかないとき](#うまくいかないとき)

## 最初に 1 回だけやること

### インストールしたい

```bash
cargo install risundle
```

(Rust ツールチェーンと `g++` などの C++ コンパイラが必要です)

### 自作ライブラリを使えるようにしたい

```bash
risundle library add mylib ~/cp/library
```

`mylib` は好きな ID、`~/cp/library` はライブラリのルート (include の起点になるディレクトリ) に読み替えてください。標準ライブラリの登録は不要です (初回バンドル時に自動登録)。

### AC Library など他人のライブラリも使えるようにしたい

自作と同じく `add` するだけです。

```bash
risundle library add ac-library ~/ac-library
```

## 提出するたびにやること

### 解答を提出用の 1 ファイルにしたい

```bash
risundle main.cpp > submission.cpp
```

`main.cpp` が実際に使っている部分だけが残った `submission.cpp` ができます。あとはジャッジに貼るだけ。

### ファイルを作らず直接クリップボードにコピーしたい

```bash
risundle main.cpp | iconv -t cp932 | clip.exe   # WSL (日本語 Windows)
risundle main.cpp | pbcopy                      # macOS
risundle main.cpp | xclip -sel clip             # Linux (X11)
risundle main.cpp | wl-copy                     # Linux (Wayland)
```

clip.exe は Windows 側のコードページ (日本語環境では cp932) で入力を解釈するため、UTF-8 のまま流すと日本語コメントが文字化けし、改行が消えてコードの意味が変わることすらあります。`iconv -t cp932` を挟んで変換してください。

### テストから提出までまとめてやりたい (oj と組み合わせる)

サンプルテストや提出の機能は、責任を絞るため risundle 自体には持たせていません。[oj](https://github.com/online-judge-tools/oj) など既存ツールと `&&` で繋いでください。

```bash
# サンプルが通ったらバンドルして、そのまま提出
oj t && risundle main.cpp > submission.cpp && oj s submission.cpp
```

`&&` は直前のコマンドが成功したときだけ次を実行するので、サンプルが落ちたら提出まで進みません。

## 提出ファイルを調整したい

### ジャッジに入っているライブラリは展開せず `#include` のまま残したい

例: AtCoder Library がジャッジ側にあるので埋め込みたくない。

```bash
risundle -k std -k ac-library main.cpp > submission.cpp
```

`-k` に渡すのは登録時の ID です。`-k` を 1 つでも指定すると既定の `keep = ["std"]` は上書きされるため、`-k std` も一緒に指定してください (忘れると標準ライブラリまで展開されます)。このとき解答側の include は `#include "..."` ではなく `#include <atcoder/dsu>` のような山括弧で書いてください (`"..."` だと keep が効かないことがあります)。

### ジャッジと同じコンパイラ・同じフラグで処理したい

```bash
risundle -c clang++ main.cpp -- -std=gnu++20 -O2
```

`-c` でコンパイラを変更、`--` より後ろはコンパイラへそのまま渡ります。`#ifdef` でコンパイラやフラグによって分岐するコードを書いているときに。

### コンパイルが通ることを確認してから提出したい (自動フォールバック)

tree-shaking は近似なので、まれに必要な定義を削ることがあります。失敗時にどう退避するかは人それぞれのため、自動フォールバック機能は risundle 自体には持たせず、シェルで組む方式にしています。バンドル後に手元でコンパイル検証し、通らなければ全展開 (`--no-tree-shaking`) に自動で切り替えるには、`||` (直前のコマンドが失敗したときだけ実行) で繋ぎます。

```bash
risundle main.cpp > submission.cpp
g++ -fsyntax-only -std=gnu++20 submission.cpp ||
  risundle --no-tree-shaking main.cpp > submission.cpp
```

`-fsyntax-only` は構文チェックだけして実行ファイルを作らないので高速です。フラグはジャッジに合わせてください。

毎回打つのが面倒なら、シェルの設定ファイル (`~/.bashrc` など) に関数として書いておけます。

```bash
bundle() {
  risundle "$1" > submission.cpp &&
    g++ -fsyntax-only -std=gnu++20 submission.cpp 2>/dev/null ||
    { echo 'tree-shaking に失敗したため全展開します' >&2 &&
      risundle --no-tree-shaking "$1" > submission.cpp; }
}
```

以後 `bundle main.cpp` だけで済みます。フォールバックが発動したら、tree-shaking の取りこぼしですので [Issue](https://github.com/TwoSquirrels/risundle/issues) で報告してもらえると助かります。

### 提出ファイルの先頭に元のソースをコメントで残したい

```bash
risundle -e main.cpp > submission.cpp
```

コードを公開するジャッジで、tree-shaking 前の読みやすいコードを見せたいときに。

## ライブラリを触ったらやること

### ライブラリを書き換えたので反映したい

```bash
risundle library update mylib   # 1 つだけ
risundle library update         # 登録済みぜんぶ
```

反映し忘れてもバンドル時に「変更された」と止まって教えてくれるので、そのとき打てば大丈夫です。

### 何を登録したか確認したい

```bash
risundle library list           # ID とパスの一覧
risundle library show mylib     # 詳細
risundle library show mylib -v  # どのファイルがどの識別子を定義しているかまで
```

tree-shaking の結果が想定と違うとき、`-v` で「この関数はこのファイル由来」を確認できます。

### 登録をやめたい

```bash
risundle library delete mylib
```

### g++ 以外のコンパイラでも標準ライブラリを解決したい

```bash
risundle library add-std clang++
```

呼ぶたびにコンパイラが追加されていくので、g++ と clang++ を両方登録して `-c` で使い分けられます。

## 毎回同じオプションを打ちたくない

解答ファイルのあるディレクトリ (かその親、たとえばプロジェクトルート) に `.risundlerc.toml` を置くと、配下のバンドル全部に効きます。

```toml
[compiler]
path = "g++"
options = ["-std=gnu++20", "-O2", "-DONLINE_JUDGE", "-DATCODER"]

[library]
keep = ["std", "ac-library"]
```

CLI オプションを併用した場合はそちらが優先されます。

## うまくいかないとき

### 「ライブラリが変更された」と言われて止まる

登録後にライブラリの中身が変わっています。反映するのが本筋、急ぐなら検証スキップ。

```bash
risundle library update            # 本筋
risundle -n main.cpp               # 急場しのぎ (検証スキップ)
```

### バンドル後のファイルがコンパイルエラーになる

まず tree-shaking を切って全部展開すれば、とりあえず提出できます。

```bash
risundle --no-tree-shaking main.cpp > submission.cpp
```

これで直る場合は tree-shaking が必要な定義を削っています ([Issue](https://github.com/TwoSquirrels/risundle/issues) で報告してもらえると助かります)。この切り替えを自動化したい場合は「[コンパイルが通ることを確認してから提出したい](#コンパイルが通ることを確認してから提出したい-自動フォールバック)」を参照してください。これでも直らない場合は、宣言と実装がファイル分割されたライブラリの可能性があります (v1.0 はヘッダーオンリーのみ対応)。

### `-k` したはずのライブラリが展開されてしまう

解答側の include を `#include "atcoder/dsu"` のような `"..."` 形式で書いていませんか。`#include <atcoder/dsu>` の山括弧形式に変えてください。

### `-k` を付けたら標準ライブラリまで展開されるようになった

`-k` の指定は既定の `keep = ["std"]` を上書きします。`-k std` を並べて指定してください。

### エラーメッセージの行番号が元のファイルと合わない

バンドル後のファイルには `#line` が入っており、コンパイラの診断は元ファイルの行を指します。ジャッジ上のエラー行番号も元ファイル基準で読んでください。
