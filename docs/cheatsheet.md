# risundle cheatsheet

English | [日本語](cheatsheet.ja.md)

A task-oriented quick reference: find what you want to do, copy the command, done. See the [README](../README.md) for what each feature is, and [spec.md](spec.md) for the fine details.

- [One-time setup](#one-time-setup)
- [Every time you submit](#every-time-you-submit)
- [Tweaking the submission file](#tweaking-the-submission-file)
- [After touching a library](#after-touching-a-library)
- [Tired of typing the same options](#tired-of-typing-the-same-options)
- [When things go wrong](#when-things-go-wrong)

## One-time setup

### Install

```bash
cargo install risundle
```

(Requires the Rust toolchain and a C++ compiler such as `g++`.)

### Make your own library available

```bash
risundle library add mylib ~/cp/library
```

`mylib` is any ID you like; `~/cp/library` is the library root (the directory your includes are relative to). No need to register the standard library — it is registered automatically on the first bundle.

### Make third-party libraries like AC Library available

Just `add` them the same way.

```bash
risundle library add ac-library ~/ac-library
```

## Every time you submit

### Bundle a solution into a single file for submission

```bash
risundle main.cpp > submission.cpp
```

You get a `submission.cpp` containing only the parts `main.cpp` actually uses. Paste it into the judge and you are done.

### Copy straight to the clipboard without creating a file

```bash
risundle main.cpp | clip.exe                    # Windows / WSL (ASCII-only sources)
risundle main.cpp | iconv -t cp932 | clip.exe   # WSL on Japanese Windows (see below)
risundle main.cpp | pbcopy                      # macOS
risundle main.cpp | xclip -sel clip             # Linux (X11)
risundle main.cpp | wl-copy                     # Linux (Wayland)
```

Caution on WSL: `clip.exe` interprets its input in the Windows system code page, not UTF-8. If your code contains non-ASCII comments, they get garbled — and line breaks can even disappear, silently changing what the code means. Convert first with `iconv -t <your code page>` as shown above (cp932 on Japanese Windows).

### Run tests and submit in one go (combine with oj)

To keep its responsibilities small, risundle itself has no test or submit features. Chain it with existing tools such as [oj](https://github.com/online-judge-tools/oj) using `&&`.

```bash
# If the samples pass, bundle and submit right away
oj t && risundle main.cpp > submission.cpp && oj s submission.cpp
```

`&&` runs the next command only if the previous one succeeded, so a failing sample never reaches the submit step.

## Tweaking the submission file

### Leave a library the judge already has as `#include`, unexpanded

Example: the judge provides the AtCoder Library, so you don't want it embedded.

```bash
risundle -k std -k ac-library main.cpp > submission.cpp
```

`-k` takes the ID you used at registration. Specifying `-k` even once overrides the default `keep = ["std"]`, so pass `-k std` alongside it (forget it and the standard library gets expanded too). Also write the includes in your solution with angle brackets like `#include <atcoder/dsu>`, not `#include "..."` (with `"..."` the keep may not take effect).

### Process with the same compiler and flags as the judge

```bash
risundle -c clang++ main.cpp -- -std=gnu++20 -O2
```

`-c` switches the compiler; everything after `--` is passed to the compiler as is. Useful when your code branches on compilers or flags with `#ifdef`.

### Verify it compiles before submitting (automatic fallback)

Tree-shaking is an approximation, so on rare occasions it can drop a needed definition. How to fall back on failure differs per person, so risundle itself has no automatic fallback — you compose it in the shell instead. To test-compile the bundle and switch to full expansion (`--no-tree-shaking`) when it fails, chain with `||` (which runs the next command only if the previous one failed).

```bash
risundle main.cpp > submission.cpp
g++ -std=gnu++20 -o /dev/null submission.cpp ||
  risundle --no-tree-shaking main.cpp > submission.cpp
```

Going all the way through linking also catches undefined references when only a definition got dropped (the binary itself is not needed, so it is discarded to `-o /dev/null`). Match the flags to your judge. Also, if you keep a library other than std, its `#include` is invisible to plain g++ — tell it where the library lives with `-I ~/ac-library` or similar (otherwise the check always fails and the fallback fires every time).

If that is too much to type every time, put a function in your shell config (`~/.bashrc` etc.).

```bash
bundle() {
  risundle "$1" > submission.cpp &&
    g++ -std=gnu++20 -o /dev/null submission.cpp ||
    { echo 'verification failed; falling back to full expansion' >&2 &&
      risundle --no-tree-shaking "$1" > submission.cpp; }
}
```

From then on, `bundle main.cpp` is all you need. When the fallback fires, check the g++ error printed right above it: a mistake in your own code means the fallback output won't compile either, while an undefined reference to a dropped definition means tree-shaking missed something — a report in the [issues](https://github.com/TwoSquirrels/risundle/issues) would be much appreciated.

### Keep the original source as a comment at the top of the submission

```bash
risundle -e main.cpp > submission.cpp
```

For judges that publish submissions, when you want readers to see the readable pre-tree-shaking code.

## After touching a library

### Reflect changes after editing a library

```bash
risundle library update mylib   # just one
risundle library update         # everything registered
```

Even if you forget, bundling stops with a "library changed" error to remind you — just run it then.

### Check what is registered

```bash
risundle library list           # IDs and paths
risundle library show mylib     # details
risundle library show mylib -v  # down to which file defines which identifiers
```

When tree-shaking output is not what you expected, `-v` tells you which file each identifier comes from.

### Unregister

```bash
risundle library delete mylib
```

### Resolve the standard library with compilers other than g++

```bash
risundle library add-std clang++
```

Each call adds another compiler to the set, so register both g++ and clang++ and switch with `-c`.

## Tired of typing the same options

Put a `.risundlerc.toml` in the directory of your solutions (or a parent, e.g. the project root) and it applies to every bundle underneath.

```toml
[compiler]
path = "g++"
options = ["-std=gnu++20", "-O2", "-DONLINE_JUDGE", "-DATCODER"]

[library]
keep = ["std", "ac-library"]
```

CLI options take precedence when both are given.

## When things go wrong

### Bundling stops saying the library has changed

The library contents changed after registration. Reflecting the change is the proper fix; skip verification if you are in a hurry.

```bash
risundle library update            # proper fix
risundle -n main.cpp               # quick and dirty (skips verification)
```

### The bundled file fails to compile

Turn off tree-shaking (everything except kept libraries gets expanded) — that gets you something submittable for now.

```bash
risundle --no-tree-shaking main.cpp > submission.cpp
```

If this fixes it, tree-shaking dropped a needed definition (a report in the [issues](https://github.com/TwoSquirrels/risundle/issues) would be much appreciated). To automate this switch, see "[Verify it compiles before submitting](#verify-it-compiles-before-submitting-automatic-fallback)". If it still fails, the library may split declarations and implementations across files (v1.0 supports header-only libraries only).

### A library you passed to `-k` gets expanded anyway

Are you writing the include as `#include "atcoder/dsu"`? Switch to the angle-bracket form `#include <atcoder/dsu>`.

### Adding `-k` made the standard library get expanded too

Specifying `-k` overrides the default `keep = ["std"]`. Pass `-k std` alongside it.

### Error line numbers don't match the original file

The bundled file contains `#line` directives, so compiler diagnostics point at the original files. Read error line numbers on the judge as referring to your original file too.
