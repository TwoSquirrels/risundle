# risundle Specification

English | [日本語](spec.ja.md)

This document goes deeper than the README, covering each command's behavior, error conditions, output format, and data structures. For the internal rationale of "why it is designed this way," see [architecture.md](architecture.md) (Japanese only).

## Basic policy

- No `#include` reduction or minification (those belong to IWYU and external minifiers, respectively). The sole goal is reducing the bundled file size.
- Tree-shaking is an approximation based on identifier-name matching, not strict dependency analysis. It errs toward keeping too much, so needed code is rarely removed by mistake, and even if something is missed, you notice it through a compile error at submission time.
- Macros are expanded. This is so that `#include`s used only for local debugging can also be subject to tree-shaking.
- Bundling multiple source files is not supported.
- Internal data such as registered libraries is stored under a `risundle` directory inside [`dirs::data_local_dir()`](https://docs.rs/dirs/latest/dirs/fn.data_local_dir.html) (hereafter `$LOCAL`). Setting the `RISUNDLE_DATA_HOME` environment variable overrides the base directory on every OS (same meaning as `XDG_DATA_HOME`).

## `library` subcommand

- `add <id> <path>` — Register `<path>` as an include path.
    - Errors if `<id>` is empty, `.`, `..`, or contains a path separator (`/`, `\`) or `:` (the ID is used as an internal data directory name as is).
    - Errors if `<id>` is `std` (use `add-std` for the standard library).
    - Errors if the same `<id>` is already registered.
    - On registration, records the list of defined identifiers for each file, the list of implementation target type names, and an aggregate hash computed from the contents under `<path>`. An implementation target is the qualifier of an out-of-class qualified definition (`X<...>::method`) or the primary template name of an explicit specialization (`template <> struct T<...>`), expressing "which type this file implements".
- `add-std [<compiler>]` — Register the standard library (`std`).
    - Auto-detects the system include search paths of `<compiler>` (default `g++`).
    - Calling it repeatedly adds that compiler to the recognized set each time (additive). It merges the search paths of all compilers in the set into one, so you can switch between multiple compilers.
- `delete <id>` — Remove a registration. Errors if not registered.
- `update [<id> [<path>]]` — Apply changes to a library.
    - When `<id>` is given, errors if not registered. For `std`, it re-detects from the recognized compiler set; otherwise it re-registers (using the registered path when `<path>` is omitted).
    - When `<id>` is omitted, updates all registered libraries.
- `list` — List the IDs and include paths of registered libraries.
- `show [-v | --verbose] <id>` — Show details of a library. Errors if not registered.
    - By default, shows the ID, include path, kind, and the number of files that have defined identifiers.
    - With `-v`, also shows the aggregate hash, the list of defined identifiers per file, and the implementation target type names.

### Update check

Every `library` subcommand checks crates.io for the latest stable release (excluding pre-releases) of risundle itself before doing its actual work, and prints a short notice to stderr if it's newer than the current version. This never happens on the bundling path (`risundle <file>`).

- The check time and latest version are cached in `$LOCAL/latest_version_cache.json`, and re-checked at most once every 24 hours. A missing, corrupt, or expired cache is all treated as "no cache" rather than an error, since it's a pure cache: if it can't be read, silently rebuilding it is enough.
- Network failures, timeouts, and malformed responses are things the user can't act on, so they're silently ignored — neither an error nor a warning.
- The suggested command adapts to the environment: `cargo install-update risundle` if `cargo-install-update` is on `PATH`, otherwise plain `cargo install risundle --force`.
- Setting the `RISUNDLE_NO_UPDATE_CHECK` environment variable disables the check entirely.

## Bundling

See the README for the full list of options. The following are the behavioral details.

### Resolving configuration

Searches from the directory of `<file>` toward its parents for `.risundlerc.toml` and uses only the single nearest one as defaults (no merging even if multiple exist). Items omitted in the configuration file are filled in with the built-in defaults (`compiler = g++`, `options = ["-std=gnu++17", "-O2", "-DONLINE_JUDGE", "-DATCODER"]`, `keep = ["std"]`, `embed = false`). Each item in the configuration file is a declarative, complete value and is not merged with the defaults (writing `keep = ["ac-library"]` does not include the default `std`).

How CLI options are layered on top depends on the type of the item.

- `compiler` (scalar) — the CLI `--compiler` overrides the configuration.
- `embed` (bool) — `--embed` / `--no-embed` are a last-wins pair, overriding the configuration only when given explicitly.
- `keep` (set) — effective keep = (configured keep ∪ `--keep`) − `--no-keep`. When the same ID appears in both, `--no-keep` wins regardless of order.
- `options` (ordered list) — effective options = configured options + the CLI options after `--`. Overriding between flags of the same kind is delegated to the compiler's own last-wins rules (`-std` etc.) and `-U`; risundle does not interpret them.

With `--no-config`, no `.risundlerc.toml` is read at all and the behavior is exactly identical to an environment where no configuration file is found. This guarantees that the effective settings can always be rebuilt from the built-in defaults through the CLI alone.

When `std` is not in the effective keep, a warning is printed (expanding `std` is almost always an accident whose huge output only surfaces at submission time). The warning is suppressed when `--no-keep std` was passed explicitly on the CLI, which is taken as intent.

A warning is also printed when the effective keep contains an ID that matches no registered library, or when a `--no-keep` ID matched neither the configured keep nor `--keep` — both are no-op instructions (a keep ID already cancelled by `--no-keep` triggers neither warning, being a resolved contradiction). `std` is excluded here, as its absence is covered by the warning above. These are not errors: the configuration file is committed to a repository while library registrations are machine-local state, so unregistered IDs occur structurally right after a clone. The warnings aggregate multiple IDs into one line, do not prevent the output from being produced, and do not change the exit code.

### Library change detection

For libraries other than `std` that are not marked to be kept (`keep`), the registration-time hash is compared against the current contents. If they differ, it prompts you to run `library update` and exits with an error. Specifying `--no-check` skips the verification itself. Kept libraries and `std` are not verified because they do not use identifier information. For the same reason, no library is verified when `--no-tree-shaking` is specified.

The hash is content-based rather than mtime-based, so it does not false-positive on time changes from `git clone` or `cp`, while it can detect file additions, deletions, and renames.

### Keep

A kept library is not expanded and is left as `#include` (excluded from tree-shaking). `std` is included by default. Keeping `std` adds `-nostdinc` to the compiler and resolves system headers through a dummy.

### Disabling tree-shaking (`--no-tree-shaking`)

With `--no-tree-shaking`, identifier detection, dependency-header reverse lookup, and the `-M` computation of the required set are all skipped, and no header is treated as unused. In other words, every library except the kept ones remains fully expanded. Because identifier tags are never consulted, library change detection is also skipped. Keep and dummy resolution behave the same as usual.

This option is intended as a temporary fallback for when tree-shaking goes wrong, and it cannot be set from `.risundlerc.toml` (allowing it in the configuration file would require a paired CLI option to turn tree-shaking back on, complicating the specification).

### Keeping implementation files

In a library that splits declarations and implementations across files, some dependencies cannot be detected as identifiers — operator overloads are the typical case (the user writes `f * g`; the token `operator*=` never appears). In addition to the identifier reverse lookup, the following rule therefore applies.

> A file whose implementation target names include a type defined by an already-needed file is also considered needed.

Whenever new files become needed, the `-M` required set is recomputed, repeating until nothing is added (the candidates are limited to files present in the output and grow monotonically, so this always terminates). The matching only considers files present in the output; implementation files that are not included are never pulled in.

Qualifiers whose target cannot be determined statically (decltype, dependent names, etc.) are not recorded and are outside this rule. Files with neither a named definition nor an out-of-class implementation (e.g. a file defining only free-function operators) are also outside it and may still be dropped by tree-shaking.

### Output format

- The first line carries the credit `// Bundled with risundle v<version>`.
- With `--embed`, the original source is attached afterward as `// ` comments.
- The body contains `#line` directives indicating origins (so that post-bundle compiler diagnostics point to the lines in the original files). Kept libraries are restored to `#include` (angle-bracket form), while other used headers remain as expanded code.
- Lines consumed by preprocessing — macro definitions, untaken `#ifdef` branches, and the like — leave blank lines behind, but a run never exceeds 8 lines. The preprocessor fills gaps longer than 8 lines with a linemarker instead of blank lines, which risundle converts into a single `#line` directive, so macro-heavy sources do not bloat the output with blank lines.

For the conditions a library must meet for risundle to bundle it correctly (per-file self-containedness and so on), see [compatibility.md](compatibility.md).

## `tags.json`

The core data structure generated by `library add` and read by the bundle command. It lives at `$LOCAL/libraries/<id>/tags.json`. It is a machine-local cache and is not portable (`path` is an absolute path).

When `<id>` is not `std`:

```json
{
  "schema_version": 2,
  "path": "/home/user/cp-library",
  "hash": "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
  "files": {
    "matrix/matrix.hpp": ["Matrix", "identity"],
    "matrix/matrix-mul.hpp": ["pow"]
  },
  "implements": {
    "matrix/matrix-mul.hpp": ["Matrix"]
  }
}
```

When `<id>` is `std`:

```json
{
  "schema_version": 2,
  "path": "/usr/include/c++/12",
  "compilers": ["/usr/bin/x86_64-linux-gnu-g++-14"]
}
```

- `schema_version`: An integer for schema compatibility checks. Registrations whose value differs from the current one are automatically regenerated from the library sources at bundle time (a cache format difference is risundle's own concern and requires no user action). `update` and `add-std` likewise read only the registered path (or the compiler set for `std`) and rebuild, so they recover even from a mismatched schema. `list` and `show` stay read-only and never regenerate, printing what they can read (`show` prints regeneration guidance in place of the details).
- `path`: The include path (absolute). For `std`, it records a representative one among the detected system include paths (the C++ standard library dir of the first compiler) for display.
- `compilers`: `std` only. The set of compilers that `std` was registered with (normalized to the absolute paths of the actual binaries).
- `hash`: An aggregate hash computed from the relative paths and contents of all files under `path`. Not present for `std`.
- `files`: Keys are paths relative to the library root, and values are the arrays of identifier names that the file defines. Not present for `std`. Non-`std` libraries always carry an empty object `{}` even when there are no identifiers, so they are structurally distinguishable from `std`.
- `implements`: Keys are paths relative to the library root, and values are the arrays of implementation target type names of the file. Files without implementation targets have no key. Not present for `std`. Treated as empty when the key is missing on load.
