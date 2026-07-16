# risundle

**A tree-shaking C++ source bundler for competitive programming**

English | [日本語](README.ja.md)

[![CI](https://github.com/TwoSquirrels/risundle/actions/workflows/ci.yml/badge.svg)](https://github.com/TwoSquirrels/risundle/actions/workflows/ci.yml)
[![coverage](https://img.shields.io/endpoint?url=https://twosquirrels.github.io/risundle/badge.json)](https://twosquirrels.github.io/risundle/)
[![crates.io](https://img.shields.io/crates/v/risundle.svg)](https://crates.io/crates/risundle)
[![license](https://img.shields.io/crates/l/risundle.svg)](LICENSE)

risundle bundles your competitive programming solution, libraries included, into a single file ready for submission. Unlike tools such as [oj-bundle](https://github.com/online-judge-tools/verification-helper), which simply expand every `#include` as is, risundle performs tree-shaking to keep only the header files your solution actually uses, so the bundled file stays small.

## Features

- A tool tailored to competitive programming submissions: it needs no heavy static analysis like IWYU and relies solely on your local compiler's preprocessing.
- It keeps only the header files your solution actually uses, so submissions pass even on judges with strict size limits.
- Prepare a single template that includes all of your own libraries, and you no longer need to switch includes per problem.

> [!NOTE]
> risundle bundles correctly only libraries with reasonably well-behaved file layouts. Libraries that split declarations and implementations across files are supported too, but a file containing only operator overloads, for example, can lose its definitions and fail to compile or link after bundling. See [docs/compatibility.md](docs/compatibility.md) for the exact conditions.

## Bundling example

For example, register a three-file library under the ID `mylib`: `modpow.hpp` and `combination.hpp` built on top of `modint.hpp`.

```cpp
// modint.hpp
#pragma once

template <long long MOD>
struct ModInt {
    long long v = 0;
    ModInt(long long v) : v(v % MOD) {}
    ModInt& operator*=(ModInt o) { v = v * o.v % MOD; return *this; }
    // ~100 more lines of implementation
};

// modpow.hpp
#pragma once
#include "modint.hpp"

template <long long MOD>
ModInt<MOD> modpow(ModInt<MOD> a, long long n) {
    ModInt<MOD> r = 1;
    for (; n > 0; n >>= 1, a *= a)
        if (n & 1) r *= a;
    return r;
}

// combination.hpp
#pragma once
#include "modint.hpp"

template <long long MOD>
struct Combination {
    // ~100 lines of implementation
};
```

The solution includes two files from this library, but only actually uses `modpow`.

```cpp
// main.cpp
#include <bits/stdc++.h>
#include <modpow.hpp>
#include <combination.hpp>

using mint = ModInt<998244353>;

int main() {
    std::cout << modpow(mint(2), 100).v << std::endl;
}
```

Bundling it produces this single file.

```cpp
// submission.cpp
// Bundled with risundle v2.1.0
#line 1 "main.cpp"
#include <bits/stdc++.h>
#line 1 "mylib/modpow.hpp"
       
#line 1 "mylib/modint.hpp"
       

template <long long MOD>
struct ModInt {
    long long v = 0;
    ModInt(long long v) : v(v % MOD) {}
    ModInt& operator*=(ModInt o) { v = v * o.v % MOD; return *this; }
    // ~100 more lines of implementation
};
#line 3 "mylib/modpow.hpp"

template <long long MOD>
ModInt<MOD> modpow(ModInt<MOD> a, long long n) {
    ModInt<MOD> r = 1;
    for (; n > 0; n >>= 1, a *= a)
        if (n & 1) r *= a;
    return r;
}
#line 4 "main.cpp"

using mint = ModInt<998244353>;

int main() {
    std::cout << modpow(mint(2), 100).v << std::endl;
}
```

The included but unused `combination.hpp` is removed, while `modint.hpp` is kept because `modpow.hpp` depends on it. The standard library stays as `#include`. The `#line` directives make compiler diagnostics point at the original files, so judge errors also read in your original line numbers.

## Installation

You need the [Rust toolchain](https://www.rust-lang.org/tools/install) and a C++ compiler with a GCC-compatible driver interface, such as `g++` or `clang++` (MSVC is not supported, as it lacks the `-E`/`-M`/`-v` interface risundle relies on).

```bash
cargo install risundle
```

You can use [cargo-update](https://crates.io/crates/cargo-update) to upgrade.

```bash
cargo install-update risundle
```

Running `risundle library` (a subcommand) prints a short notice to stderr when a newer version is available (bundling itself, i.e. running `risundle` directly, never does). Set `RISUNDLE_NO_UPDATE_CHECK` to disable it.

## Quick start

```bash
# Register your own library (the ID is arbitrary)
risundle library add mylib ~/cp/library

# Bundle a solution that includes the registered library into a single file
risundle main.cpp > submission.cpp
```

`std` is registered automatically on the first bundle and is kept by default.

## Usage

For a task-oriented "how do I ..." reference, see [docs/cheatsheet.md](docs/cheatsheet.md). The following describes each feature.

### Bundling

```
risundle [OPTIONS] <FILE> [-- <COMPILER OPTIONS>...]
```

Bundles `<FILE>` and writes the result to standard output.

| Option | Description |
| --- | --- |
| `-c`, `--compiler <PATH>` | Compiler to use (default: `g++`) |
| `-k`, `--keep <ID>` | Also keep a library unexpanded, out of tree-shaking (repeatable; default: `std`) |
| `--no-keep <ID>` | Stop keeping a library (repeatable; beats `--keep`) |
| `--no-tree-shaking` | Disable tree-shaking and expand everything except kept libraries (useful as a fallback) |
| `-e`, `--embed` | Embed the original source as a comment at the top |
| `--no-embed` | Do not embed the original source (cancels a configured `embed = true`) |
| `-n`, `--no-check` | Skip the hash verification of library updates |
| `--no-config` | Ignore any `.risundlerc.toml`, behaving as if none exists |
| `-- <OPTIONS>...` | Pass everything after `--` straight to the compiler, appended to the configured options |

`--keep` leaves a library unexpanded as an `#include`, whereas `--no-tree-shaking` expands every library except the kept ones but performs no tree-shaking. The two are different and can be combined. Note that `--no-tree-shaking` also skips the hash verification of library updates, since it uses no identifier information.

```bash
# Use clang++ and leave AC Library unexpanded too (std stays kept by default)
risundle -c clang++ -k ac-library main.cpp > submission.cpp

# Pass extra options to the compiler
risundle main.cpp -- -std=gnu++20 -O2
```

### Library management

```
risundle library <SUBCOMMAND>
```

| Subcommand | Description |
| --- | --- |
| `add <ID> <PATH>` | Register a library |
| `add-std [COMPILER]` | Register the standard library (`std`) (default: `g++`) |
| `list` | List registered libraries |
| `show <ID> [-v]` | Show details of a library |
| `update [ID] [PATH]` | Apply changes to a library (updates all libraries when ID is omitted) |
| `delete <ID>` | Remove a library registration |

`add-std` can be called multiple times. Adding a compiler with, for example, `risundle library add-std clang++` merges each one's system includes so you can switch between them.

## Configuration file

risundle searches from the directory of the solution file toward its parents and adopts the single nearest `.risundlerc.toml` (it does not merge multiple files). Explicit CLI options take precedence over the configuration file: scalars and booleans are overridden, `--keep` and `--no-keep` add and remove kept libraries, and options after `--` are appended to the configured ones. `--no-config` ignores the file entirely.

```toml
[compiler]
path = "g++"
options = ["-std=gnu++17", "-O2", "-DONLINE_JUDGE", "-DATCODER"]

[library]
keep = ["std"]

[bundle]
embed = false
```

The above are the default values. Omitted items are filled in with these defaults.

## Benchmarks

We compared execution time against IWYU (include-what-you-use 0.21). The environment was WSL 2 (Ubuntu 24.04, Intel Core 7 240H, g++ 14.2).

| Library | risundle | IWYU |
| --- | --- | --- |
| AC Library | 0.031 s | 0.491 s |
| Nyaan's Library | 0.033 s | 2.085 s |

risundle stays nearly constant regardless of library size, while IWYU grows as the number of headers increases. This is because IWYU fully builds the clang AST, whereas risundle relies solely on the compiler's preprocessing (`-E`/`-M`). Note that IWYU and risundle serve different purposes (IWYU suggests `#include` fixes; risundle bundles) and do not solve the same problem.

## How it works

1. Expand includes via preprocessing (`-E`). Libraries marked to be kept (`keep`) are left as `#include` by routing them through a dummy.
2. Detect the identifiers your solution uses through lexical analysis, and reverse-look-up the dependent headers from the definitions of registered libraries.
3. Compute the transitive closure of required headers with `-M` (also keeping the implementation files of needed types), and remove the unneeded headers left in the output.
4. Reassemble everything into a single file while preserving the original origins with `#line` directives.

Because include expansion is delegated to the compiler, both `#pragma once` and manual include guards are handled correctly. See [docs/spec.md](docs/spec.md) for the detailed behavior and error conditions of each command.

## Development

The functional specification is in [docs/spec.md](docs/spec.md), the conditions a library must meet are in [docs/compatibility.md](docs/compatibility.md), and the internal design rationale is in [docs/architecture.md](docs/architecture.md) (Japanese only).

## License

[MIT License](LICENSE) — © 2026 TwoSquirrels
