# risundle

**A tree-shaking C++ source bundler for competitive programming**

English | [日本語](README.ja.md)

[![CI](https://github.com/TwoSquirrels/risundle/actions/workflows/ci.yml/badge.svg)](https://github.com/TwoSquirrels/risundle/actions/workflows/ci.yml)
[![coverage](https://img.shields.io/endpoint?url=https://twosquirrels.github.io/risundle/badge.json)](https://twosquirrels.github.io/risundle/)
[![crates.io](https://img.shields.io/crates/v/risundle.svg)](https://crates.io/crates/risundle)
[![license](https://img.shields.io/crates/l/risundle.svg)](LICENSE)

risundle bundles your competitive programming solution, libraries included, into a single file ready for submission. Unlike tools such as [oj-bundle](https://github.com/online-judge-tools/verification-helper), which simply expand every `#include` as is, risundle performs tree-shaking to keep only the parts your solution actually uses, so the bundled file stays small.

## Features

- A tool tailored to competitive programming submissions: it needs no heavy static analysis like IWYU and relies solely on your local compiler's preprocessing.
- It keeps only the code your solution actually uses, so submissions pass even on judges with strict size limits.
- Prepare a single template that includes all of your own libraries, and you no longer need to switch includes per problem.

> [!WARNING]
> v1.0 supports header-only libraries only. Libraries that split declarations and implementations across separate files may lose their definitions after bundling and fail to compile.

## Installation

You need the [Rust toolchain](https://www.rust-lang.org/tools/install) and a C++ compiler (such as `g++`).

```bash
cargo install risundle
```

You can use [cargo-update](https://crates.io/crates/cargo-update) to upgrade.

```bash
cargo install-update risundle
```

## Quick start

```bash
# Register your own library (the ID is arbitrary)
risundle library add mylib ~/cp/library

# Bundle a solution that includes the registered library into a single file
risundle main.cpp > submission.cpp
```

`std` is registered automatically on the first bundle and is kept by default.

## Usage

### Bundling

```
risundle [OPTIONS] <FILE> [-- <COMPILER OPTIONS>...]
```

Bundles `<FILE>` and writes the result to standard output.

| Option | Description |
| --- | --- |
| `-c`, `--compiler <PATH>` | Compiler to use (default: `g++`) |
| `-k`, `--keep <ID>` | Library ID to exclude from tree-shaking (repeatable; default: `std`) |
| `--no-tree-shaking` | Disable tree-shaking and expand everything (useful as a fallback) |
| `-e`, `--embed` | Embed the original source as a comment at the top |
| `-n`, `--no-check` | Skip the hash verification of library updates |
| `-- <OPTIONS>...` | Pass everything after `--` straight to the compiler |

`--keep` leaves a library unexpanded as an `#include`, whereas `--no-tree-shaking` expands every library but performs no tree-shaking. The two are different.

```bash
# Use clang++ and leave AC Library unexpanded, keeping it as #include
risundle -c clang++ -k std -k ac-library main.cpp > submission.cpp

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

risundle searches from the directory of the solution file toward its parents and adopts the single nearest `.risundlerc.toml` (it does not merge multiple files). CLI options take precedence over the configuration file.

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

We compared execution time against IWYU (include-what-you-use 0.21). The environment was WSL2 (Ubuntu 24.04, Intel Core 7 240H, g++ 14.2).

| Library | risundle | IWYU |
| --- | --- | --- |
| AC Library | 0.031 s | 0.491 s |
| Nyaan Library | 0.033 s | 2.085 s |

risundle stays nearly constant regardless of library size, while IWYU grows as the number of headers increases. This is because IWYU fully builds the clang AST, whereas risundle relies solely on the compiler's preprocessing (`-E`/`-M`). Note that IWYU and risundle serve different purposes (IWYU suggests `#include` fixes; risundle bundles) and do not solve the same problem.

## How it works

1. Expand includes via preprocessing (`-E`). Libraries marked to be kept (`keep`) are left as `#include` by routing them through a dummy.
2. Detect the identifiers your solution uses through lexical analysis, and reverse-look-up the dependent headers from the definitions of registered libraries.
3. Compute the transitive closure of required headers with `-M`, and remove the unneeded headers left in the output.
4. Reassemble everything into a single file while preserving the original origins with `#line` directives.

Because include expansion is delegated to the compiler, both `#pragma once` and manual include guards are handled correctly.

## Development

The functional specification is in [docs/spec.md](docs/spec.md), and the internal design rationale is in [docs/architecture.md](docs/architecture.md) (Japanese only).

## License

[MIT License](LICENSE) — © 2026 TwoSquirrels
