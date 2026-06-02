# risundle

**Tree-Shaking 機能付き、競技プログラミング用 C++ ソースバンドラー**

**© 2026 TwoSquirrels**  
ライセンス: [MIT License](LICENSE)

競技プログラミングの解答を、ライブラリ込みで提出用の 1 ファイルにまとめるツールです。[oj-bundle](https://github.com/online-judge-tools/verification-helper) などの既存のバンドラと違い、解答で実際に使っている部分だけを残す Tree-Shaking を行うため、バンドル後のファイルサイズが小さくなります。

> [!WARNING]
>
> 現在制作中です。検討中の仕様案は [docs/draft.md](docs/draft.md) をご覧ください。

## アピールポイント

oj-bundle などの既存バンドラは include をそのまま展開するので提出ファイルが大きくなりがちですが、risundle は使っている部分だけを残すので小さく収まります。`atcoder/modint` を入れただけで引っかかるような、AOJ・yukicoder の厳しい提出サイズ制限でも安心です。

巨大な自作ライブラリを全部 include したテンプレートを 1 つ用意するだけでよく、問題ごとに include を切り替える必要もありません。余計なコードが減るぶん、コンパイルも速くなります。

## 開発

コードに手を入れる際は、以下の検討資料に目を通してください。

- [docs/architecture.md](docs/architecture.md) — 内部設計の方針 (依存の向き・モジュール構成・判断の理由)
- [docs/draft.md](docs/draft.md) — v1.0 の機能仕様案

どちらも v1.0 完成後に削除・改訂予定です。
