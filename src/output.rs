//! 標準出力への書き込みを一箇所に集約する。`print!`/`println!` は書き込み失敗時にパニックするが、
//! パイプ先が `head` やページャで早期に閉じるだけの broken pipe はユーザー操作として普通に起こり得る
//! ため、パニックにはせず黙って正常終了する。

use std::io::{ErrorKind, Write as _};

/// 改行を付けずにそのまま書き込む。バンドル出力のような、それ自体で完結したテキスト向け。
pub fn write(text: &str) {
    write_bytes(text.as_bytes());
}

/// 末尾に改行を付けて書き込む。`println!` の代替。
pub fn write_line(text: &str) {
    write_bytes(format!("{text}\n").as_bytes());
}

fn write_bytes(bytes: &[u8]) {
    if let Err(err) = std::io::stdout().write_all(bytes) {
        if err.kind() == ErrorKind::BrokenPipe {
            std::process::exit(0);
        }
        panic!("failed to write to stdout: {err}");
    }
}
