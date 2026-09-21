# Small Lisp

A tree-walking interpreter for the Small Lisp language specification, written in Rust.

## Run

```sh
cargo run -- examples/smoke.lisp
cargo run -- examples/onboarding.lisp
```

The executable accepts a source-file path. Without a path, it reads the program from standard input.

```sh
cargo run -- program.lisp
cargo test
```

The implementation includes lexical scopes and mutable closures, recursive fixed-arity functions, explicit aliases (`^name`), arrays and insertion-ordered structs, checked integer arithmetic, integer bitwise operators (`bit-and`, `bit-or`, `bit-xor`, `bit-not`, `bit-shl`, `bit-shr`), formatting, regex capture matching, loop control flow, and file-descriptor IO.

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

## File IO

`io/open` accepts `fopen`-style mode strings: `r`, `r+`, `w`, `w+`, `a`, and `a+`. It returns an integer file descriptor. `io/read` reads one UTF-8 line without its EOL and returns `_` at EOF; `io/write` writes a string and returns its byte count; `io/close` returns `t` when it closes an open descriptor and `f` otherwise.

Descriptors `0`, `1`, and `2` are open at startup for standard input, standard output, and standard error. Files opened through `io/open` receive descriptors beginning at `3`. Bind descriptors with a `#`-prefixed name and use that name with `io/read`, `io/write`, and `io/close`.

```lisp
(let #fd (io/open "w" "greeting.txt"))
(io/write #fd ($ "Hello, %s\\n" "world"))
(io/close #fd)
```

## HTTP

`@` performs synchronous HTTP requests with `GET`, `POST`, `PUT`, `PATCH`, `DELETE`, and `HEAD`:

```lisp
(@ "https://example.com" "GET")
(@ "https://example.com/api" "POST" {"name":"Yvan"})
(@ "https://example.com/api" "POST" "hello") ; sends the JSON string "hello"
```

JSON responses are converted to Lisp values when the response `Content-Type` is
`application/json`; other responses are returned as strings. `HEAD` always returns an empty
string. HTTP errors, unsupported methods, connection failures, and invalid JSON are reported as
normal Lisp errors.

`$` supports `%s` (rendered value), `%d` (decimal integer), `%b` (binary integer), `%8b`, `%16b`, `%32b`, and `%64b` (fixed-width binary integers), `%h` (lowercase hexadecimal integer), `%o` (octal integer), `%f` (number), `%j` (JSON), `%t` (value type), `%v` (structural debug output), and `%%` (a literal percent sign). Fixed-width binary forms render the low bits of the integer, including two's-complement representations for negative integers.

`expect` checks that its first expression equals its second expression and returns `t`. An optional final string describes the assertion when it fails.

```lisp
(expect (add 1 1) 2 "addition works")
```

`~` matches a Rust UTF-8 regex and binds its full match plus capture groups to the supplied
identifier. It returns `f` without changing the binding when there is no match.

```lisp
(let string "Hello the world")
(~ "^Hello(.*)$" string match)
; ["Hello the world" " the world"]
```

Strings support the same 1-based indexing and inclusive slices as arrays. Positions count
Unicode grapheme clusters, so an emoji or a combining-character sequence is one element.
Single indexes and ranges return strings; multi-index selectors return arrays of strings.

```lisp
(let s "😀abc")
s[1]       ; "😀"
s[2]       ; "a"
s[1..2]    ; "😀a"
s[[1 3]]   ; ["😀" "b"]
```

`examples/onboarding.lisp` is a small interactive program showing those pieces working together. It reads a name from standard input, validates it, mutates a profile through an alias, and performs a countdown.
