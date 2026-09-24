# Small Lisp

A small tree-walking Lisp interpreter written in Rust. It provides lexical scopes, mutable
closures, arrays, structs, Unicode-aware strings, regex captures, HTTP, and file-descriptor IO.

The language is documented in [`spec.txt`](spec.txt). Runnable examples are in
[`examples/`](examples/).

## Requirements

- Rust and Cargo (stable toolchain)

## Quick start

```sh
cargo test
cargo run -- examples/smoke.lisp
cargo run -- examples/onboarding.lisp
```

Build an optimized executable with:

```sh
cargo build --release
./target/release/small-lisp examples/smoke.lisp
```

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

- lexical scopes, mutable closures, recursion, and explicit aliases (`^name`)
- heterogeneous arrays and insertion-ordered structs
- checked integer arithmetic and integer bitwise operations
- fixed-arity user functions and variadic numeric folds
- conditional expressions, pattern matching, loops, `break`, and `continue`
- value formatting with `$`, equality and comparison operators, and `expect` assertions
- UTF-8 strings indexed by Unicode grapheme clusters
- regular-expression matching with capture arrays
- synchronous HTTP requests and file-descriptor IO

Native modules are loaded with `use`. The initial registry provides `str`, whose functions use
the same descriptor representation as Lisp-defined callable fields:

```lisp
(use "str")
(str.upper "hello") ; "HELLO"
(str.lower "HELLO") ; "hello"
($ "%s" str.upper.spec.documentation)
```

Function arity belongs to each callable. User-defined functions are fixed-arity, while
`add`, `mul`, `sub`, `div`, comparisons, and the three bitwise folds are variadic. There is no
automatic currying.

```lisp
(add)             ; 0
(add 1 2 3 4)     ; 10
(mul 2 3 4)       ; 24
(sub 10 3 2)      ; 5
(div 20 2 2)      ; 5
(bit-or 1 2 4)    ; 7
```

### Arrays and strings

Array indexes are 1-based and support negative indexes. Ranges are inclusive and may omit either
bound. String positions follow the same rules, but count Unicode grapheme clusters rather than
bytes or Unicode scalar values.

```lisp
(let values [10 20 30 40 50])
values[1]       ; 10
values[-1]      ; 50
values[2..4]    ; [20 30 40]

(let text "😀abc")
text[1]         ; "😀"
text[1..2]      ; "😀a"
text[[1 3]]    ; ["😀" "b"]
```

Single string indexes and ranges return strings. Multi-index selectors return arrays of strings.

### Regex captures

`~` uses Rust's UTF-8 `regex` engine. Its syntax is
`(~ regex string binding)`. On success it returns an array containing the full match followed by
the capture groups, and binds that array to the supplied identifier. An unmatched optional group
is represented by `_`. On failure it returns `f` and leaves an existing binding unchanged.

```lisp
(let string "Hello the world")
(~ "^Hello(.*)$" string caps)
; ["Hello the world" " the world"]

(let caps ["unchanged"])
(~ "^Goodbye" string caps)
; f; caps is still ["unchanged"]
```

## File IO

Load the native `io` module with `(use "io")`. `io.open` accepts file URIs whose `mode` query parameter is one of `r`, `r+`, `w`, `w+`, `a`, or `a+`. It returns an integer file descriptor. `io.read` reads one UTF-8 line without its EOL and returns `_` at EOF; `io.write` writes a string and returns its byte count; `io.close` returns `t` when it closes an open descriptor and `f` otherwise.

Descriptors `0`, `1`, and `2` are open at startup for standard input, standard output, and standard error. Files opened through `io.open` receive descriptors beginning at `3`. Bind descriptors with a `#`-prefixed name and use that name with `io.read`, `io.write`, and `io.close`.

```lisp
(use "io")
(let fd (io.open "file:greeting.txt?mode=w"))
(io.write fd ($ "Hello, %s\\n" "world"))
(io.close fd)
```

## HTTP

`@` performs synchronous HTTP requests with `GET`, `POST`, `PUT`, `PATCH`, `DELETE`, and `HEAD`:

```lisp
(@ "https://example.com" "GET")
(@ "https://example.com/api" "POST" {name:"Yvan"})
(@ "https://example.com/api" "POST" "hello") ; sends the JSON string "hello"
```

JSON responses are converted to Lisp values when the response `Content-Type` is
`application/json`; other responses are returned as strings. `HEAD` always returns an empty
string. HTTP errors, unsupported methods, connection failures, and invalid JSON are reported as
normal Lisp errors.

## Formatting and assertions

`$` supports `%s` (rendered value), `%d` (decimal integer), `%b` (binary integer), `%8b`, `%16b`, `%32b`, and `%64b` (fixed-width binary integers), `%h` (lowercase hexadecimal integer), `%o` (octal integer), `%f` (number), `%j` (JSON), `%t` (value type), `%v` (structural debug output), and `%%` (a literal percent sign). Fixed-width binary forms render the low bits of the integer, including two's-complement representations for negative integers.

`expect` checks that its first expression equals its second expression and returns `t`. An optional final string describes the assertion when it fails.

```lisp
(expect (add 1 1) 2 "addition works")
```

`examples/onboarding.lisp` is a small interactive program showing those pieces working together. It reads a name from standard input, validates it, mutates a profile through an alias, and performs a countdown.

## License

No license has been declared yet.
