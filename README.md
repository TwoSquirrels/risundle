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

> [!NOTE]
> Libraries that split declarations and implementations across files are handled by tracing implementation files through the "implementation target type names" recorded at registration. Files whose target cannot be determined statically (e.g. files defining only free-function operators) may still lose their definitions after bundling and fail to compile.

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

Because include expansion is delegated to the compiler, both `#pragma once` and manual include guards are handled correctly.

## Development

The functional specification is in [docs/spec.md](docs/spec.md), and the internal design rationale is in [docs/architecture.md](docs/architecture.md) (Japanese only).

## License

[MIT License](LICENSE) — © 2026 TwoSquirrels
