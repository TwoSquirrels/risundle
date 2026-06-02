# risundle 設計方針

draft.md が機能仕様 (何を作るか) を定めるのに対し、本書は内部設計の方針 (どう作るか) を残す。コードからは読み取りにくい判断の記録で、draft.md と同じく v1.0 が完成すれば役目を終える。

## 依存は内向き一方向

クリーンアーキテクチャ (原義) が言うのは「ソースの依存を一方向に保て」だけで、同心円の図や層の数・名前は一例にすぎない。安定したもの (ライブラリという概念や Tree-Shaking) に、揮発的なもの (tree-sitter ・ コンパイラ ・ JSON ・ clap) が依存する形にし、逆流させない。層を切る基準は「将来差し替わるか」の一点。

## モジュール構成

```
src/
  main.rs            配線・argv 振り分け
  cli.rs             clap 定義
  config.rs          .risundlerc.toml の読み取り
  commands/          サブコマンドのハンドラ (library.rs / bundle.rs)
  library/           登録済みライブラリのドメイン
    tags.rs          tags.json のデータと永続化
    local.rs         $LOCAL 上の配置 (LocalStore)
    hash.rs          内容ベースの集約ハッシュ
    dummy.rs         維持ライブラリのダミー生成
    identifiers.rs   tree-sitter による識別子抽出
  fs/                汎用ファイル走査 (walk / source / relpath)
  bundle/            (v1.0 実装予定) Tree-Shaking とコンパイラ連携
```

依存は `commands → bundle → library → fs` の内向き一方向で、循環しない。

- `fs` は何にも依存しない走査ユーティリティで、`library` と `bundle` が共有する。
- `library` は走査を `fs` に委ね、登録済みライブラリの表現・永続化・登録処理を担う。
- `bundle` は `library` の tags.json を逆引きに使うため `library` に依存する。
- `identifiers` ・ `dummy` は登録専用なので `library` 内に置く。`bundle` は使わない。

## 規模に合わせて、あえてやらないこと

CLI ツール (約 2000 行) では、抽象を足すほど薄いディレクトリと配線が増えてかえって読みにくい。依存方向さえ守れば足りるので、次は入れない。

- `domain` ・ `infra` のような抽象的な層名を作らず、ドメイン名でそのまま分ける。
- trait と DI は入れない。tree-sitter やコンパイラの差し替えが現実の要求になったら、その境界にだけ足せばよい。
- tags.rs はデータ構造と JSON 読み書きを分けない。同居の方が読みやすく、schema_version でスキーマ進化を吸収できている。

## Tree-Shaking は過剰検出側に倒す

C++ の厳密な依存解析は難しいので、識別子名の照合で近似する。余分に拾う分には安全 (取りこぼしだけが依存漏れ = コンパイルエラーになる) なので、メンバ名やマクロ名まで拾っても気にしない。

例外は namespace 名だけ。`atcoder` のような名前はほぼ全ファイルに現れ、定義として登録すると逆引きで大半のヘッダーが依存扱いになって Tree-Shaking が効かなくなる。過剰検出が裏目に出る唯一のケースなので、namespace 名は集めない (中のメンバは個別に拾う)。詳細は identifiers.rs の `NAME_NODES` を参照。
