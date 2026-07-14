# What Libraries risundle Can Bundle

English | [日本語](compatibility.ja.md)

C++ `#include` is plain text expansion: however you split code across files, it is valid as long as it compiles. risundle, however, analyzes each library file individually to decide whether it is needed, so it cannot handle every possible split. Only libraries with reasonably well-behaved file layouts are supported.

Stating the exact boundary of "well-behaved" — a precise necessary-and-sufficient condition for the tool to work — is hard. This document therefore gives a sufficient condition close to it: follow this, and nothing breaks.

That said, an ordinary header-only library — one where each file opens and closes its own namespace and writes named definitions plainly — already meets every condition below without doing anything. AC Library and most major competitive programming libraries have this shape.

Also, these conditions only apply to libraries that get expanded (not kept). A kept library is neither expanded nor tree-shaken and stays as `#include`, so a library that violates the conditions is still fine to use as long as the judge has it and you keep it. See the [README](../README.md) for how to register libraries and keep them.

## Premise: risundle only removes, never adds

Bundling means "expand the original solution as is and remove the unneeded headers"; risundle never hunts down and supplies anything missing. Everything therefore rests on the premise that **the pre-bundle solution compiles and links as is, given the library locations via `-I`**. A solution that does not compile will not compile after bundling either. Also, a solution must be a single source file; multiple .cpp files cannot be bundled together (splitting the solution into headers you include is still one source file and does not hit this limit, but see "Use library code in the solution file itself" below).

One pitfall follows directly from this premise. In a library that splits declarations and implementations across files, **the implementation files must also be included by the solution**. Given a solution that includes only the declaration header, risundle will not guess and add the implementation file (that solution fails to link before bundling, which falls outside the premise). Pulling in implementation files automatically via symbol reachability is envisioned in [#10](https://github.com/TwoSquirrels/risundle/issues/10), but is not planned for now.

## Each file must be a complete C++ declaration on its own

At `library add` time, risundle parses each library file individually, without preprocessing, and records the names it defines (classes, functions, variables, and so on). At bundle time, it reverse-looks-up these records from the names appearing in your solution and keeps only the needed files. Each file is therefore required to:

- **Parse on its own.** Any scope a file opens (`{`, `namespace`, etc.) must be closed within the same file, and a file must not start in the middle of a scope another file opened.
- **Carry, as a name, the clue for its own survival.** It must contain either a named definition or an out-of-class implementation (whose target type name is picked up; see "Keeping implementation files" in [spec.md](spec.md)). A file with neither can only be judged "unused" and may be dropped by tree-shaking.

Typical layouts that break these conditions follow.

### A scope opens and closes across files

If you split a function mid-body as below, `back.hpp` carries no name, so it is removed, leaving the opening brace in `front.hpp` unclosed — a compile error.

```cpp
// front.hpp
int solve() {

// back.hpp
  return 42;
}
```

Wrapping another file's `#include` in a namespace breaks for the same reason.

```cpp
// wrapper.hpp
namespace mylib {
#include "impl.hpp"
}

// impl.hpp
struct Vector {};
```

`wrapper.hpp` itself carries no name and may be removed entirely, leaving the contents of `impl.hpp` emitted at global scope without `namespace mylib`. If the solution writes the qualified `mylib::Vector`, a compile error reveals the problem; but with unqualified use such as `using namespace mylib;`, the code may silently compile with its meaning changed.

### Macros generate declaration names

The registration-time analysis does not preprocess, so it sees the source text as written, not macro expansion results. In a layout where macros generate type or function names (so-called X-macros), the generated names are never recorded.

```cpp
#define DEFINE_POINT(Name) struct Name { int x, y; };
DEFINE_POINT(Point)
```

Registering this file records `Point` nowhere, so even a solution that uses `Point` cannot establish that the file is needed, and the file gets removed.

If you really want macro-generated names, write the generated name as a plain forward declaration in the same file.

```cpp
#define DEFINE_POINT(Name) struct Name { int x, y; };
DEFINE_POINT(Point)
struct Point;
```

A forward declaration parses as written, so `Point` gets recorded and the file survives. Redundant declarations are harmless in C++, so the library behaves the same. This works beyond classes: function declarations (`int f(int);`) and variable declarations (`extern int x;`) do the job as well.

Platform branching with `#ifdef` is fine, by the way. Because there is no preprocessing, as long as each branch is a complete declaration on its own, the names of both branches are recorded on the safe side regardless of which one ends up taken.

### How breakage shows up

When a file disappears because these conditions were violated, the result is almost always a compile error or a link error.

- When the syntax itself breaks (the function-split example) or a used definition is lost outright (the X-macro example), you get a compile error.
- When the declaration survives on the class side and only the implementation goes missing (implementation files whose target cannot be determined statically — e.g. a file defining only free-function operators), you get a link error (undefined reference). Symbolic operators carry no name, so such files cannot be saved by forward declarations.

The one exception is the namespace-wrapper example: with unqualified use, the meaning can silently change without any error.

Note that out-of-class implementations and explicit specializations only work as clues when the target type (primary template) is defined in an expanded file of a registered library. A file whose target lives outside the library (e.g. one containing only a `std::hash` specialization) can likewise disappear, but since the whole specialization is lost, it shows up as a compile error rather than a link error.

Compile and link errors can be caught by verifying locally before submitting; see the automatic-fallback recipe in [cheatsheet.md](cheatsheet.md) (a shell one-liner that compiles the bundle locally and falls back to full expansion on failure). The permanent fix is restructuring the library: for example, operator definitions survive when placed in the same file as a named definition (or the file with the class body). In a pinch, `--no-tree-shaking` gets you through.

## Use library code in the solution file itself

At bundle time, the lines examined to determine "what the solution uses" are limited to the solution file itself. If a library is used only inside an unregistered local header of yours, that dependency goes undetected and may be removed by mistake. The local header itself is outside tree-shaking and stays fully expanded; it is the library header used inside it that disappears. Until this is fixed, you can work around it by placing the code that touches the library in the solution file. This is tracked as [#60](https://github.com/TwoSquirrels/risundle/issues/60) — a fixable bug, not a design limitation.

## Tree-shaking works per file

Keep-or-remove decisions are made per file (header), not per line or per member. Even if a file mixes used and unused definitions, the whole file stays. If you want smaller output, the finer you split your library into files, the better tree-shaking works.

## Other notes

- **A file named `library` cannot be bundled as is.** A leading `library` argument is always interpreted as the library-management subcommand, so spell it as a path, like `risundle ./library`.
- **Include kept libraries with the `<>` form.** If you write `#include "..."` and a path of the same name (e.g. `./atcoder/dsu`) happens to exist in the solution file's directory, the standard C++ behavior of `""` searching the current directory first — which cannot be suppressed from the compiler — picks that one, the keep has no effect, and the library gets expanded.

## What this document leaves out

The conditions here are design limitations inherent to the record-names-and-reverse-look-up mechanism and will not be fixed (except [#60](https://github.com/TwoSquirrels/risundle/issues/60), noted above as fixable). Known bugs slated to be fixed exist separately, but since they will eventually be fixed, they are not listed here; see the [`Kind/Bug`](https://github.com/TwoSquirrels/risundle/issues?q=is%3Aissue+state%3Aopen+label%3AKind%2FBug) label.
