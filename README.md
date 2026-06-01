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
