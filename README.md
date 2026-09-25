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

- lexical scopes, mutable closures, recursion, and explicit aliases (`^name`)
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

### Arrays and strings

Array indexes are 1-based and support negative indexes. Ranges are inclusive and may omit either
bound. String positions follow the same rules, but count Unicode grapheme clusters rather than
bytes or Unicode scalar values.

### Regex captures

`~` uses Rust's UTF-8 `regex` engine. Its syntax is
`(~ regex string binding)`. On success it returns an array containing the full match followed by
the capture groups, and binds that array to the supplied identifier. An unmatched optional group
is represented by `_`. On failure it returns `f` and leaves an existing binding unchanged.

### Runtime eval

`eval` parses a string as a program and runs it in the caller's environment, returning the last
form's value — so it can name variables, execute inline code, or run source stored in a variable.
Control flow (`break`/`continue`) propagates out of the evaluated code.

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

`$` supports `%s` (rendered value), `%q` (string as a parseable Lisp literal), `%x` (any value as parseable Lisp source), `%d` (decimal integer), `%b` (binary integer), `%8b`, `%16b`, `%32b`, and `%64b` (fixed-width binary integers), `%h` (lowercase hexadecimal integer), `%8h`, `%16h`, `%32h`, and `%64h` (fixed-width hexadecimal integers), `%o` (octal integer), `%f` (number), `%j` (JSON), `%t` (value type), `%v` (structural debug output), and `%%` (a literal percent sign). Fixed-width binary forms render the low bits of the integer, including two's-complement representations for negative integers.

`expect` checks that its first expression equals its second expression and returns `t`. An optional final string describes the assertion when it fails.

## License

No license has been declared yet.
