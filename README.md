# Small Lisp

A small tree-walking Lisp interpreter written in Rust. It provides lexical scopes, mutable
closures, arrays, structs, Unicode-aware strings, regex captures, HTTP, and file-descriptor IO.

The language is documented in [`spec.txt`](spec.txt).

## Requirements

- Rust and Cargo (stable toolchain)

## Quickstart

The executable accepts a source-file path. Without a path, it reads the complete program from
standard input until EOF:

```sh
printf '(add 20 22)\n' | cargo run --quiet
```

Scripts may start with a `#!` shebang line, so they can be executed directly after
`chmod +x`:

```sh
#!/usr/bin/env small-lisp
(expect (add 20 22) 42)
```

## Language overview

Small Lisp includes:

- lexical scopes, mutable closures, recursion (bounded: exceeding the call-depth limit is a
  clean `RecursionError` with file:line:col and the call trace, never a stack crash), and
  explicit aliases (`^name`)
- heterogeneous arrays and insertion-ordered structs
- checked integer arithmetic and integer bitwise operations
- fixed-arity user functions and variadic numeric folds
- conditional expressions, pattern matching, loops, `break`, and `continue`
- value formatting with `$`, equality and comparison operators, and `expect` assertions
- runtime evaluation of source strings with `eval`
- UTF-8 strings indexed by Unicode grapheme clusters
- regular-expression matching with capture arrays
- synchronous HTTP requests and file-descriptor IO
- Native modules are loaded with `use`.

### Module descriptors

Modules expose callable descriptors of the form `{_: <callable> spec: {...}}`. The `spec`
carries `documentation` (string), `arity` (integer), `type` (array — or `_` when `arity` is 0)
and `return` (array or `_`). `type` is one flat signature of exactly `arity` entries bound
positionally to the parameters; each entry is either a single type name or a non-empty array of
alternative type names, checked independently per argument:

```lisp
spec: { documentation: "add" arity: 2
        type: [["int" "float"] "string"]   ; arg 1 ∈ {int, float}, arg 2 = string
        return: ["int"] }
```

The type vocabulary is `null | bool | int | float | string | array | struct | function | ref |
any`. `"any"` is the top/wildcard type: it accepts every value type and must appear **only** as
a standalone entry (`type: ["any"]`) — combining it with alternative types in a set
(`type: [["int" "any"]]`) is a registration error, never silently normalized to `"any"`.
Unknown type names, empty alternative sets, and sets containing `"any"` are rejected when the
module is registered, never silently accepted as never-matching declarations. `arity` stays a
single fixed integer and `type` length must always equal it: alternatives are per-argument only (the
allowed combinations are the cross product of the per-argument sets), never whole-signature
overloads. `return` is declaration-only and not enforced at call time.

A Lisp module is defined with `let` and registered with `use`, which validates its syntax:

```lisp
(let mymodule {
    greet: {_: (fn (name) ($ "Hello %s!" name))
            spec: {documentation: "Greets a person." arity: 1 type: ["string"] return: ["string"]}}
})
(use "mymodule")
(mymodule.greet "Yvan")          ; "Hello Yvan!"
```

`use "name"` loads a native module (`io`, `str`, `http`), binding it like `let` (a duplicate in
the same scope is `DuplicateBindingError`); for a user-defined module it looks up the existing
binding, validates the module syntax, and registers it — at most once per scope
(`ModuleError` otherwise, with shadowing in a child scope registering afresh).

### Arrays and strings

Array indexes are 1-based and support negative indexes. Ranges are inclusive and may omit either
bound. String positions follow the same rules, but count Unicode grapheme clusters rather than
bytes or Unicode scalar values.

### Regex captures

Regex matching happens inside `$` formatting: the `%~` specifier applies a regex (with options)
to an argument, and `%M.C` / `%N` emit whole matches and capture groups (see Formatting above).
The engine is Rust's UTF-8 `regex`. An unmatched optional group emits `_`; no match at all emits `f`.
Regexes match Unicode scalar values, not grapheme clusters: `(.)` on "👍🏽" (base emoji +
skin-tone modifier, two scalars) captures just the base "👍", while string indexing is
grapheme-based and returns the whole cluster.

### Runtime eval

`eval` parses a string as a program and runs it in the caller's environment, returning the last
form's value — so it can name variables, execute inline code, or run source stored in a variable.
Control flow (`break`/`continue`) propagates out of the evaluated code.

### Debugging REPL

`repl` pauses the running program at the exact point of the call and drops into an interactive
session with full access to the live state:

```lisp
(let port 8080)
(io.write 1 "about to listen…\n")
(repl "debug")
(io.write 1 ($ "listening on %d\n" port))
```

Every line is evaluated as Lisp in a throwaway child scope: `set` mutates program variables,
`let` binds only inside the session, and the last non-null result is echoed. The prompt is
contextual — `prog.lisp:7> ` pinpoints the `(repl)` call site (`label@prog.lisp:7> ` when the
session has a label). Commands: `:c`/`:continue` resumes the program, `:q`/`:quit` aborts it,
`:h`/`:help` lists commands. Debugging commands are resolved against the live program:
`:l`/`:list [N]` shows the source lines (±3 by default, `:l 0` = the execution line only,
marked `>`) around the execution point, `:i`/`:inspect` lists the effective bindings in scope
(`:i name` shows one binding's value, type, binding scope and definition site, with the shadow
chain when one exists), and `:bt`/`:backtrace` dumps the call stack innermost-first with
`file:line:col` positions. A Ctrl-C while a session is open cancels the current line and keeps
you in the session; the pre-session state is restored on resume, so Ctrl-C after `:c` still
aborts the run. Errors inside a line are reported without stopping the program,
`(break)`/`(continue)` act on the enclosing loop, and nested `(repl)` sessions work. The
session reads from and writes to the process's controlling terminal (`/dev/tty`), so program
stdin is never consumed — even `cat program.lisp | small-lisp` reaches an interactive session,
and `(io.read 0)` keeps reading the original stdin. Without a controlling terminal, `(repl)`
raises an IO error instead of falling back to stdin; EOF on the terminal ends the session
like `:c`.

## Values, aliases, and references

Small Lisp uses value semantics with explicit aliasing. Arrays, structs, and nested composites
are **immutable snapshots** shared for reads: any read — a variable lookup, `.`/`[...]` access, a
function argument, a `let` — yields the *current* value and can never mutate it. Mutation only
ever replaces the value stored in a cell (a variable root), and a rebind is invisible to earlier
readers.

`^` produces a **reference**: a logical location — the root cell plus a path of field/index
steps — never a value snapshot. A reference is resolved against the *current* value at each
read and write:

- `(let a [1 2]) (let p (^ a[1]))` — `p` reads as `1`, and after `(set a[1] 9)` reads as `9`.
- Rebinding the root re-targets the alias: `(set a [5 6])` makes `(^ a[1])` resolve to `5`, and
  rebinding with an **incompatible** type (`(set a {x:1})`) fails deterministically on read with
  a clean type error (`indexing requires an array`) — never a stale value. The same alias resolves
  again once the location exists.
- `(let q p)` dereferences and copies the *current* value (fully isolated); only `(let q (^ p))`
  creates a second live alias (two-hop chains work).
- References stored *inside* an array/struct stay live for reads, but writing to the final
  position just **replaces the alias** (`(set box.p 7)` writes 7 into `box.p`). Variables holding
  a reference — and intermediate path steps — **write through**: with `a[1] = (^ b)`,
  `(set a[1][2] 9)` mutates `b`.
- Argument passing is by value (snapshot); `(g ^x)` makes the parameter an alias into the caller.
- Assigning a value whose refs point at an ancestor of the target location — directly, indirectly,
  or through a call — creates a cycle and is rejected with `cyclic reference`.
- `eq` dereferences transparently and compares current values, as do `%s`, `%q`, `%x`, and `%j`
  (`%s` degrades an invalid ref to `null`/`<invalid reference>`; `%x`/`%j` raise a type error).
  `%v` keeps the `Ref(...)` wrapper explicit so cyclic structures stay renderable, and `%t` on a
  reference yields `ref`.

**Implementation note (performance).** The model is deliberately simple: reads are O(1) `Rc`
shares of immutable snapshots (the major win), while each write rebuilds the snapshot chain along
the path — a full array/struct copy per level, O(children) per level — so writes into large or
deeply nested composites are the residual cost. Moving to a fancier persistent structure (e.g. a
32-way vector trie) is explicitly **out of scope for this implementation** and stays deferred
unless a *realistic write-heavy workload* shows `set` consuming more than ~30–40% of runtime;
microbenchmarks alone are not sufficient justification. The `Rc<Vec>` model is correct,
well-tested, and already delivers the targeted speedup.

## File IO

Load the native `io` module with `(use "io")`. `io.open` accepts file URIs whose `mode` query parameter is one of `r`, `r+`, `w`, `w+`, `a`, or `a+`. It returns an integer file descriptor. `io.read` reads one UTF-8 line without its EOL and returns `_` at EOF; `io.write` writes a string and returns its byte count; `io.close` returns `t` when it closes an open descriptor and `f` otherwise.

Descriptors `0`, `1`, and `2` are open at startup for standard input, standard output, and standard error. Files opened through `io.open` receive descriptors beginning at `3`. 
Each open file owns a buffered reader that lives in the descriptor for its whole lifetime, so
repeated `io.read` calls on one descriptor reuse the same buffer and do not lose data the reader
prefetched; the buffered stream stays bound to the descriptor for future reads. In `r`, reads
only; in `r+`, `w+`, and `a+` the descriptor is both readable and writable, and in `r+`/`w+` a
write lands at the current stream position while in `a+` it appends to the end of the file.
`io.read` on a write-only descriptor, on `stdout`, or on `stderr` produces an IO
error (e.g. `file descriptor 1 is not readable`); `io.write` on a read-only descriptor or on
`stdin` produces an IO error.

## HTTP

The `http` module performs synchronous HTTP requests with `GET`, `POST`, `PUT`, `PATCH`, `DELETE`, and `HEAD`:

```lisp
(use "http")
(http.get "https://example.com")
(http.post "https://example.com/api" {name:"Yvan"})
(http.post "https://example.com/api" "hello") ; sends the JSON string "hello"
```

JSON responses are converted to Lisp values when the response `Content-Type` is
`application/json` or any media type with a `+json` suffix (e.g. `application/problem+json`,
per RFC 6839); other responses are returned as strings. `HEAD` always returns an empty
string. HTTP errors, connection failures, and invalid JSON are reported as normal Lisp errors.

## Formatting and assertions

`$` supports `%s` (rendered value), `%q` (string as a parseable Lisp literal), `%x` (any value as parseable Lisp source), `%d` (decimal integer), `%b` (binary integer), `%8b`, `%16b`, `%32b`, and `%64b` (fixed-width binary integers), `%h` (lowercase hexadecimal integer), `%8h`, `%16h`, `%32h`, and `%64h` (fixed-width hexadecimal integers), `%o` (octal integer), `%f` (number), `%j` (JSON), `%t` (value type), `%v` (structural debug output), and `%%` (a literal percent sign). Fixed-width binary forms render the low bits of the integer, including two's-complement representations for negative integers. `%~` applies a regex to a second argument — the regex takes the form `[options]~pattern`, where options are letters from `gmisxUuR` (default `gmu`; `g` finds all matches, `m` line anchors, `i` case-insensitive, `s` dot matches newlines, `x` ignored whitespace, `U` ungreedy, `u` unicode, `R` CRLF line endings) — computing all matches without emitting anything itself. `%M.C` then emits capture `C` (`C=1` = whole match, `C=2` = first capture, …) of match `M`, and `%N` is shorthand for `%1.N` (`%1` = whole first match, `%2` = first capture, …). No match emits `f`; a missing optional capture emits `_`.

`expect` checks that its first expression equals its second expression and returns `t`. An optional final string describes the assertion when it fails.

## License

No license has been declared yet.
