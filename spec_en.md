# Small Lisp Specification

Small Lisp is a minimal interpreted Lisp. The interpreter is a tree-walking
evaluator written in Rust. The language is meant for simple IO scripting.

This document is the reference for the language as it is actually implemented,
not as it was intended to be. Where the French original `spec.txt` disagrees with
the code, this document follows the code and Appendix C lists every such change.
Every statement below was either checked against the running interpreter or read
directly from the implementation. Where a behaviour is surprising, it is
described plainly rather than called a bug, so the document stays a description
of what the interpreter does.

Terms used here:

- *form* means any single piece of source code.
- *value* means a runtime datum.
- *binding* means a name attached to a storage cell in a scope.
- *snapshot* means an immutable value shared for reading (see Section 7).

---

## Table of contents

1. Types
2. Names, keywords and built-ins
3. Grammar
4. let and set
5. Scopes
6. Functions
7. References
8. Arithmetic
9. Coercions
10. eq by type
11. if, match, and, or, not
12. loop, break, continue
13. IO
14. HTTP
15. Interpolation
16. Regex
17. eval
18. Errors
19. Structs
20. REPL
21. Implementation
Appendix A. Built-in functions
Appendix B. Format specifiers
Appendix C. Differences from spec.txt

---

## 1. Types

| Type        | Notation                                    | Internal |
| ----------- | ------------------------------------------- | -------- |
| string      | `"..."`                                     | String   |
| raw string  | `'...'`, no escapes                        | String   |
| bool        | `t` / `f`                                   | bool     |
| integer     | `[+-]?` then decimal, `0x` hex, `0b` binary, `0o` octal | i64 |
| float       | `N.N`                                       | f64      |
| null        | `_`                                         | unit     |
| array       | `[e1 e2 ...]`, heterogeneous, nestable     | Vec      |
| struct      | `{k1: v1 k2: v2 ...}`, identifier keys only | ordered map |

### 1.1 Number literals

An integer literal is an optional sign followed by:

- decimal digits, or
- `0x` and at least one hexadecimal digit, or
- `0b` and at least one binary digit, or
- `0o` and at least one octal digit.

The prefixes are lowercase only. `0X10` is not a number, it is the name `0X10`.
There is no octal shorthand: `017` is the decimal number 17. A sign is allowed on
a prefixed literal, so `-0x10` is -16.

An integer must fit in a signed 64 bit value. A decimal literal outside that
range names the offending value in the message, while a radix literal does not:

| Literal | Error |
| ------- | ----- |
| `99999999999999999999` | `ParseError: integer out of range \`99999999999999999999\`` |
| `0xFFFFFFFFFFFFFFFF` | `ParseError: integer out of range` |
| `0o7777777777777777777777` | `ParseError: invalid number \`0o7777777777777777777777\`` |

The radix form is read into an unsigned 64 bit value first, so
`0xFFFFFFFFFFFFFFFF` is out of range, and the octal literal with 22 digits
exceeds even 64 unsigned bits and fails the parse entirely.

A prefix with no digit after it, such as `0x`, is not a number at all. It is the
name `0x`.

A float literal is a sign, then digits, then a dot, then digits. All three parts
are required:

- `.5` is a `ParseError: unexpected token Dot`.
- `5.` is a `ParseError: expected struct field after dot`.
- `1e3` is not a float, it is the name `1e3`.
- `1_000` is not a number, it is the name `1_000`.

Because there is no exponent notation, a very large or very small float cannot be
written as a literal. The whole number is still read, so
`100000000000000000000.0` is accepted and is 1e20.

Five float literals are predefined: `NaN`, `+NaN`, `Inf`, `+Inf` and `-Inf`.
There is no `-NaN` literal, `-NaN` is an ordinary name and gives a `NameError`.

### 1.2 Strings

A `"..."` string understands exactly these escapes:

| Escape | Character |
| ------ | --------- |
| `\n`   | line feed |
| `\r`   | carriage return |
| `\t`   | tab |
| `\"`   | double quote |
| `\'`   | single quote |
| `\\`   | backslash |

Any other backslash sequence is not an error. The backslash is dropped and the
character after it is kept. So `"\q"` is the string `q`, `"\0"` is the string `0`,
`"\x41"` is the string `x41`, and `"\u{1f600}"` is the string `u{1f600}`.

A `'...'` raw string has no escapes at all, so `'ab\ncd'` is the eleven
characters `ab`, a backslash, `n`, `cd`. There is no way to embed a single quote,
and a raw string cannot be adjacent to another token: `'it''s'` is two raw
strings, so using it where one value is expected gives an `ArityError`.

An unterminated string is a `ParseError`.

### 1.3 Arrays and structs

- Array indices are 1 based. `arr[1]` is the first element, `arr[-1]` is the last.
  `arr[0]` is a `TypeError`.
- Slices are inclusive: `arr[start..end]`. Either bound may be omitted:
  `arr[..end]`, `arr[start..]`, `arr[..]`.
- After negative bounds are normalised, `start` greater than `end` is a
  `TypeError`. `arr[3..1]` and `arr[-1..1]` are both errors. `arr[-1..-1]` is the
  last element.
- A bound of `0` is a `TypeError`, including when it is the omitted side:
  `arr[..0]` and `arr[0..]` are errors.
- An index outside the array is a `NameError`.
- `arr[[i j]]` selects several elements. It returns a new array, in the order
  given. Duplicates are kept: `a[[2 1 2]]` returns the second element, then the
  first, then the second again.
- Every slice and every multi-select returns a new array value.
- Struct keys must be a single unquoted name. `{"a": 1}` is a `ParseError`, and so
  is `{1: 2}` and `{a b: 1}`. A duplicate key is a `DuplicateKeyError`. Insertion
  order is preserved. A key may use any name characters, so `{a/b: 1}` is fine.
- Commas are optional whitespace inside `[]` and `{}`. `[1, 2]`, `[1 2]` and
  `[1,,2]` are the same array. In fact `,` is whitespace everywhere in the
  language, not only inside brackets.

A slice or a multi-select must be the last step of a postfix chain, because
neither can be a location on a path:

```
(let a [1 2 3])
a[1..2]        ; [1 2]
a[1..2][1]     ; TypeError: a slice cannot be a reference target
a[1..2].x      ; TypeError: a slice cannot be a reference target
a[[1 2]][[1]]  ; TypeError: a multi-index selector cannot be a reference target
```

### 1.4 Strings are indexed too

Strings use the same indexing as arrays:

- `str[i]` returns a string.
- `str[start..end]` returns a string.
- `str[[i j]]` returns an array of strings.
- Negative indices and negative slice bounds work as they do for arrays.
- `str[0]` is a `TypeError`. An index outside the string is a `NameError`.

String positions are Unicode grapheme clusters, not bytes and not Unicode
scalars. Take a string whose bytes are `65 cc 81 78`, that is the letter `e`
followed by the combining acute accent U+0301 and then `x`. That is two
grapheme clusters and three Unicode scalars, so:

```
s[1]     ; the whole cluster e + U+0301
s[1..1]  ; the same single cluster, a string
s[2]     ; x
s[[1 2]] ; an array of two strings
s[3]     ; NameError: string index 3 out of bounds
```

The same string illustrates the difference from Section 16: a regex `.` or a
regex group `(.)` matches only the `e`, because the engine counts scalars.

```
($ "%~%1.1" "(e.)" s)   ; the whole cluster e + U+0301, the group spans two scalars
($ "%~%1.1" "(.)"  s)   ; e only
```

### 1.5 Truthiness

Exactly four values are false:

- the integer `0`
- the float `0.0`
- `f`
- `_`

Everything else is true, including the empty string `""`, the empty array `[]`
and the empty struct `{}`.

---

## 2. Names, keywords and built-ins

### 2.1 Identifiers

A name is any run of characters that is not whitespace and does not contain one
of these:

```
(  )  [  ]  {  }  :  ,  .  ^  "  '  ;  #
```

So names may use letters, digits, `-`, `_`, `/`, `+`, `=`, `*`, `!`, `?`, `<` and
other symbols. `a/b`, `a=b` and `1a` are valid names.

`#` is not a comment character. Nothing in the language treats it specially, so
it ends the name and is then rejected on its own:
`ParseError: unexpected \`#\``.

The lexer tries to read a number first. If that fails it falls back to reading a
name. So `0x`, `0b`, `0o`, `1e3` and `1_000` are all names, not numbers. `0X10`
is a name too, because the numeric prefixes are lowercase only.

A `let` name and a `fn` parameter name must be a name token. A string, a number
or a literal in that position is a `TypeError: let name must be an identifier`.
A name that merely looks like a path, such as `(let a.b 1)`, is refused the same
way, because the lexer stops the name at the dot.

### 2.2 Reserved names

These names are literals. They cannot be used as a `let` name or as a function
parameter:

`t`, `f`, `_`, `NaN`, `+NaN`, `Inf`, `+Inf`, `-Inf`.

`let` reports `TypeError: let name must be an identifier` for them, because the
lexer did not produce a name at all.

These names are special forms and strict built-ins. They cannot be used as a
`let` name or as a function parameter:

```
let  set  if    fn     loop   break continue match and  or
not  expect use  eval   $      add   sub  mul  div  mod  pow
eq   ne   lt    gt     le     ge    bit-and bit-or bit-xor
bit-not bit-shl bit-shr repl
```

`let` reports `TypeError: reserved name cannot be bound: <name>`.
`fn` reports `TypeError: reserved name cannot be used as a parameter: <name>`.

These names are only recognised by their exact spelling when they appear at the
head of a call. Because they cannot be bound, no binding can ever shadow one.

### 2.3 Special forms

The lexer produces no special token for keywords. They are lexed as ordinary
names and recognised when they appear at the head of a call:

`let`, `set`, `if`, `fn`, `loop`, `break`, `continue`, `match`, `and`, `or`,
`not`, `expect`, `use`, `eval`, `repl`, and `$`.

These forms control when their arguments are evaluated, so not all arguments are
necessarily evaluated before the call. `eval` is described in Section 17, `repl`
in Section 20.

None of these names is an ordinary binding. Reading one as a value is a
`NameError`, so `(let q add)` fails. `add 1 2` works because the name appears at
the head of a call.

### 2.4 Strict built-ins

```
add  sub  mul  div  mod  pow
eq   ne   lt   gt   le   ge
bit-and  bit-or  bit-xor  bit-not  bit-shl  bit-shr
```

Their arguments are all evaluated first, then the operation is applied. See
Appendix A.

### 2.5 The interaction built-in

`$` performs interpolation. It is described in Section 15.

### 2.6 Native modules

Three native modules exist. They are loaded by name with `use`:

| Module | Members |
| ------ | ------- |
| `str`  | `str.upper`, `str.lower` |
| `io`   | `io.open`, `io.read`, `io.write`, `io.close` |
| `http` | `http.get`, `http.post`, `http.put`, `http.patch`, `http.delete`, `http.head` |

```
(use "str")
(str.upper "hello")   ; "HELLO"
(str.lower "HELLO")   ; "hello"
```

`str.upper` uses the Unicode uppercase mapping, so `str.upper "strasse"` returns
`"STRASSE"`.

Both members have `arity` 1, `type: ["string"]` and `return: ["string"]`.

`io` is described in Section 13, `http` in Section 14.

### 2.7 use

```
(use "name")
```

In `use name`, the expression `name` is evaluated normally and its value must be
a string. A non-string value gives `TypeError: expected string`, so an unbound
name such as `(use str)` is a `TypeError`, not a `NameError`, and a struct bound
to a name is also a `TypeError`. A Lisp module is therefore registered by its
name written as a string: `(let m <struct>)` then `(use "m")`.

The form `(use (let name value))` no longer exists. The definition (`let`) and
the registration (`use`) are two separate steps.

If the name is a native module, it is built and then bound in the current scope
exactly like `let`. A name already bound in that same scope gives a
`DuplicateBindingError` and is never overwritten. Only `set` (Section 4) changes
an existing binding.

Otherwise the name must denote an existing binding created by `(let name {...})`.
`use` then validates the syntax of the module and registers it in the current
scope. An unknown name gives `NameError: module \`name\` is not defined`.

Registration of a Lisp module is per scope. The same name can be registered only
once per scope; a second registration in the same scope gives
`ModuleError: module \`name\` is already registered`. Shadowing a parent scope
registration is allowed, exactly like `let`.

A native module is bound in the current scope, so using it twice in the same
scope is a `DuplicateBindingError`, while using it once more in a nested scope
succeeds.

A module defined with `let` stays callable through its binding with or without
`use`. `use` is the gate for syntax validation and for registration. An invalid
module is refused at registration time, not when it is called.

The validation errors, all `TypeError`, are:

| Message | Cause |
| ------- | ----- |
| `module must be a struct` | the module value is not a struct |
| `module members must be callable descriptors` | a member is not a struct |
| `module descriptor must contain _` | the `_` field is absent |
| `module descriptor _ must be callable` | `_` is not a function, or `_` nests |
| `module descriptor must contain a spec field` | the `spec` field is absent |
| `module descriptor spec must be a struct` | `spec` is not a struct |
| `module descriptor spec.documentation must be a string` | wrong `documentation` |
| `module descriptor spec.arity must be an integer` | `arity` is not a non-negative integer |
| `module descriptor spec.type must be an array, or _ for arity 0` | wrong shape for `type` |
| `module descriptor spec.type length must match spec.arity` | the lengths differ |
| `module descriptor spec.type entries must be strings or arrays of strings` | an entry is a number, a bool, a struct and so on |
| `module descriptor spec.type sets must not be empty` | an empty alternative set |
| `module descriptor spec.type set members must be strings` | a non-string in a set |
| `module descriptor spec.type \`any\` cannot be combined with alternative types` | `any` inside a set |
| `module descriptor spec.type entry \`X\` must name a known type` | an unknown type name |
| `module descriptor spec.return must be an array or null` | `return` is another type |

On every call, the arity and the types are checked again and give:

| Message | Cause |
| ------- | ----- |
| `ArityError: module function expects N arguments, got M` | wrong number of arguments |
| `TypeError: module function argument I expects T, got U` | wrong argument type |
| `TypeError: module function argument I expects one of A, B, got U` | wrong argument type, set form |

### 2.8 Module shape

Every callable member of a module must be a descriptor:

```
{_: callable spec: {...}}
```

`spec` must contain:

| Field | Type |
| ----- | ---- |
| `documentation` | string |
| `arity` | a non-negative integer |
| `type` | `_` when `arity` is 0, otherwise an array of exactly `arity` entries |
| `return` | an array or `_` |

`type: []` is refused even when `arity` is 0. The two accepted shapes are
`type: _` for `arity: 0`, and an array for `arity` greater than 0.

```
(let m {f: {_: (fn () 7) spec: {documentation: "d" arity: 0 type: _ return: []}}})
(use "m")
(m.f)   ; 7
```

`type` is one signature, not an overload set. Each entry is either a single type
name or a non-empty array of type names. An array entry means the argument is
accepted if it matches any one of those types, independently of the other
arguments. The allowed combinations are the cartesian product of the sets. It is
never a per-signature overload, `arity` stays one single integer, and the length
of `type` must always equal `arity`.

The type name vocabulary is:

```
null  bool  int  float  string  array  struct  function  ref  any
```

`"any"` is the wildcard constraint. It accepts any argument type and must appear
alone as its own entry (`type: ["any"]`). Combining it with alternatives in a
set, such as `type: [["int" "any"]]`, is refused at registration. An unknown
name, an empty set, and a set containing `"any"` are all refused at
registration.

`return` is declarative only. Its shape is validated, and it is never applied to
the value the function actually returns.

The spec fields are validated when the module is registered, and `arity` and
`type` are also enforced on every call.

A native module is an ordinary struct. Its callable members use a `_` field and
a `spec` field, exactly like Lisp descriptors.

### 2.9 Descriptors

Only a struct that is a complete descriptor, meaning it has both a `_` field
and a `spec` field, is callable. A call unwraps the value of `_`, checks that it
is callable, in exactly one level, and then applies `spec`.

A `_` field on its own never makes a struct callable and is never unwrapped.
There are therefore no chains and no cycles of `_`.

```
(let m {f: {_: (fn (x) x) spec: {documentation: "d" arity: 1 type: ["any"] return: []}}})
(use "m")
(m.f 1)   ; 1
```

Without the `use`, the same call still works: `(m.f 1)`. `use` validates the
module and records the registration.

---

## 3. Grammar

```
form          := call | block | array | struct | literal | field-access | ref
head          := name ("." name | "[" selector "]")*
call          := "(" head form* ")"
block         := "(" form* ")"
array         := "[" form* "]"
struct        := "{" (name ":" form)* "}"
field-access  := name ("." name)*
ref           := "^" postfix-access
postfix-access:= postfix ("[" selector "]" | "." name)*
selector      := form | form ".." form | "[" form* "]"
```

- `()` is the only scope constructor. `if` and `match` open a scope only when
  their branch is itself a `()`.
- `.` is the only way to read a struct field. Arrays are indexed only with
  brackets: `arr[1]`, `arr[-1]`, `arr[2..5]`, `arr[[1 3]]`.
- A parenthesised form is a **call** when its head is a name, optionally
  followed by `.name` or `[index]` steps. A name in head position is always a
  call, even when it is bound to something that cannot be called, which gives
  `TypeError: value is not callable`. A head that is a literal, an array, a
  struct or a parenthesised form makes the whole form a **block** instead, so
  `(5 1)`, `(1.5 2)`, `(t 1)`, `(_ 1)`, `([1] 1)`, `({a: 1} 1)` and
  `((fn (x) x) 2)` are all blocks that evaluate every form and return the last
  one, here `2`.
- A block creates a new scope and returns the value of its last form.
- `,` is whitespace everywhere, so `[1, 2]` and `[1 2]` are the same array.
- `;` starts a comment that runs to the end of the line. One `;` is enough.

### 3.1 Access paths must start at a variable

A field access always needs a variable at its root. `({a: 1}).a` is a
`TypeError: reference target must be a variable or field`.

An index chain of two or more links also needs a variable at its root:

```
[[1 2] [3 4]][1][2]        ; TypeError: reference target must be a variable or field
(let o [[1 2] [3 4]]) o[1][2]   ; 2, fine
```

A single index may apply to any expression, so `[1 2][1]` and `"abc"[1]` work.

The same rule applies to `^`. `(^ [1 2][1])` is a `TypeError`, because a
reference target must be a variable or a field path.

Bind to a variable first when a path is deeper than one link.

---

## 4. let and set

Both forms evaluate their value expression strictly.

`let` creates a new binding in the current scope, meaning the nearest enclosing
`()` or the top level of the program. If the name is already bound in that same
lexical scope the result is a `DuplicateBindingError`. There is no silent
rebinding. Shadowing a binding from a parent scope is allowed.

`set` changes an existing binding, walking outward through the scope chain until
it finds the name. It never creates a binding. An unknown name gives a
`NameError`.

Reading a variable dereferences any reference it holds (Section 7). So `let`
binds the current value of the location, not the location itself:

```
(let q p)          ; copies the current value of p, fully isolated
(let q (^ p))      ; a second live alias to the same location
```

`set` through an alias mutates the target location.

Both `let` and `set` evaluate to `_`.

### 4.1 Writing through a path

`set` accepts a variable, a field, an array element, or a chain of them.

```
(let b {p: 1})  (set b.p 2)  b      ; {p: 2}
(let a [1 2])   (set a[1] 9)  a      ; [9 2]
(set b.q 2)                          ; NameError: field q
(set a[5] 9)                         ; NameError: array index 5 out of bounds
(set a[0] 9)                         ; TypeError: array indices are 1-based
```

Writing the last step of a path replaces the value at that step, unless that
step holds a reference. Then it writes through (Section 7).

---

## 5. Scopes

Only `()` introduces a new lexical scope. The top level of the program is also a
scope.

`loop` creates one scope when it is entered and reuses it on every iteration.
There is no new scope per turn. This is what makes `(set x (sub x 1))` carry
over from one turn to the next. As a consequence, a `let` that is replayed on
every turn is a duplicate and gives a `DuplicateBindingError`. To bind new
variables on every turn, open a `()` in the body of the loop.

`fn` creates a new scope on every call, in which the parameters are bound. That
scope's parent is the lexical scope where the `fn` was written. This is lexical
scoping, not dynamic scoping.

Closures capture the environment by reference, so `set` inside a closure really
does mutate the outer variable. That is also what makes recursion work: the
name resolves at call time, not at definition time.

---

## 6. Functions

Arity is a property of each callable. Functions created by `fn` have a fixed
arity. Too few or too many arguments gives an `ArityError` at the call. Built-ins
may be fixed or variadic, as listed in Appendix A.

There is no automatic currying.

Arguments are passed by value. A parameter receives the current value, that is a
read-shared immutable snapshot that is isolated for writing (Section 7).

Passing `^x` makes the argument an alias of `x`, the same logical location. A
`set` in the body of the function then mutates the caller's variable. Reading
the parameter still yields the current value.

```
(let incr (fn (n) (set n (add n 1))))
(let x 5)
(incr ^x)   ; x becomes 6
(incr x)    ; a local copy is incremented, x stays 6
```

A function body is only evaluated when it is called, and the closure references
its environment by pointer, so recursion needs no `letrec`:

```
(let fact (fn (n) (if (eq n 0) 1 (mul n (fact (sub n 1))))))
(fact 5)   ; 120
```

### 6.1 fn with an immediate call

`fn` with more than two arguments does not produce a function. It builds a
function from the parameter list and the first body form, then calls it with the
remaining forms as arguments and returns the result.

The first form after the parameter list is the body. Every remaining form is an
argument. So:

```
(fn (b) (add b 1) 41)        ; 42
(fn (x y) (add x y) 1 2)     ; 3
(fn () 7)                    ; a function of arity 0, not the number 7
(fn (b) (add b 1))           ; a function of arity 1
(fn (b) 1 2 3 4 5)           ; ArityError: function expects 1, got 4
```

There is no other way to call a function value. Putting one in the head of a
call does not work, because a call head must be a name, as in Section 3. The
form below is a block, so it builds the function, throws it away, and returns
`41`:

```
((fn (b) (add b 1)) 41)      ; 41, not 42
```

The frame name in a call trace for an immediate call is
`<anonymous function>`, because the function was never given a name.

To write a function with a body of several forms, wrap the body in a `()`:

```
(fn (b) ((let c 1) (add b c)) 41)   ; 42
```

### 6.2 Recursion limit

Recursion is bounded to 2048 nested calls. Beyond that the evaluator raises:

```
RecursionError: call depth limit (2048) exceeded — is this function recursing without a base case?
```

The dash in that message and the ellipsis in the trace line below are the
non-ASCII characters U+2014 and U+2026. They are part of the output.

This is an ordinary language error. It is positioned on the faulty call with the
usual `file:line:col` and caret, and it is accompanied by the live call trace.
The trace lists the 40 innermost frames and then a line of the form
`  … N more frame(s) omitted`.

The interpreter runs on a thread with an explicit 256 MiB stack, so the guard
and not the native stack bounds the recursion. The value 2048 is calibrated
against measured native stack use per call.

For unbounded iteration use `loop`. Note that `(loop)` with an empty body never
ends.

---

## 7. References

`^` produces an alias to a location, which is a root cell plus a path of field
and element steps. An alias is never a value and never a snapshot. It is
resolved against the current value at every read and every write.

These forms are valid:

```
^identifier
^struct.field
^arr[i]
^arr                 ; the whole array
```

These are not:

```
^arr[1..2]           ; TypeError: a slice cannot be a reference target
^arr[[1 2]]          ; TypeError: a multi-index selector cannot be a reference target
^[1 2]               ; TypeError: reference target must be a variable or field
^5                   ; same TypeError
```

### 7.1 The value model

Arrays and structs are immutable snapshots shared for reading. Every read, by
value, as an argument, or through `let`, returns the current value without
offering a mutable view. A mutation only replaces the snapshot in the root cell
(Section 4).

```
(let a [1 2])
(let p (^ a[1]))
(set a [5 6])
p    ; 5, the alias follows the new value
```

If a replacement makes the path invalid, resolution fails deterministically. It
never returns stale data and never falls back to the old value. The same alias
works again as soon as the location becomes valid.

| What broke | Error |
| ---------- | ----- |
| the base of a field step is not a struct | `TypeError: struct field access requires a struct` |
| the base of an index step is not an array | `TypeError: indexing requires an array` |
| the struct has no such field | `NameError: field <name>` |
| the array index is out of range | `NameError: array index N out of bounds` |

```
(let o {a: {b: [1 2]}})
(let r (^ o.a.b[2]))
(set o {a: 5})      ; TypeError: struct field access requires a struct
r
```

```
(let o {a: {b: [1 2]}})
(let r (^ o.a.b[2]))
(set o {a: {c: 1}}) ; NameError: field b
r
```

### 7.2 Reading and copying

Reading a binding that holds a reference dereferences it:

```
(let user {name: "Yvan"})
(let n (^ user.name))
(set n "New")
user.name   ; "New"
```

`(let q p)` copies the current value of `p`, with a fully isolated binding. Only
`(let q (^ p))` creates a second live alias to the same location. A parameter
received by value is also dereferenced on read. To mutate the target from inside
a function, pass `^`.

### 7.3 References inside composites

A reference stored in a struct field or an array element stays live for reads.
Reading the field or element yields the current value.

Writing the terminal position that holds a reference replaces the reference
itself. This is leaf semantics:

```
(let box {p: 1})
(set box.p 7)   ; box.p is 7, the old reference target is untouched
```

A variable that holds a reference, and every intermediate step of a path, are
written through:

```
(let a [1 2])
(let b [3 4])
(set a[1] (^ b))
(set a[1][2] 9)
b   ; [3, 9], b was mutated
```

### 7.4 Whole array alias

`^arr` on a whole array is allowed and makes in-place mutation from a function
possible. The array passed by reference points at the same underlying structure
the caller has, so any mutation through that parameter is visible to the caller.

```
(let a [1 2 3])
(let p (^ a))
(set a[1] 9)
p   ; [9 2 3]
```

### 7.5 Cycles

Assigning a value that contains a reference pointing, directly or indirectly,
including through a chain of aliases, back to an ancestor of the target location
creates a cycle. The assignment is refused with `TypeError: cyclic reference`:

```
(let a [1])
(set a[1] (^ a))          ; TypeError: cyclic reference

(let a [1])
(let c (^ a))             ; an alias to the whole array, not to a[1]
(set a[1] c)              ; fine, c does not point into a[1]
(set a[1] (^ c))          ; TypeError: cyclic reference

(let a {p: [1]})
(set a.p (^ a))           ; TypeError: cyclic reference
```

### 7.6 Comparing and serialising references

`eq`, `%s`, `%q`, `%x` and `%j` all dereference and operate on the current value.
`%v` is the exception: it keeps the wrapper.

A reference can only be unresolved if it is still stored somewhere, because every
read of a variable dereferences it. So the cases below need a reference nested
inside a composite:

```
(let inner [1 2])
(let box [0])
(set box[1] (^ inner[1]))
(set inner 5)      ; ^ inner[1] is now unresolved
```

| Expression | Result |
| ---------- | ------ |
| `($ "%s" box)` | `[null]` |
| `($ "%v" box)` | `Array([Ref(<invalid>)])` |
| `($ "%x" box)` | `TypeError: indexing requires an array` |
| `($ "%j" box)` | `TypeError: indexing requires an array` |
| `(eq box box)` | `f` |
| `box[1]` | `TypeError: indexing requires an array` |

At the top level a `^` expression yields a reference without dereferencing, so
`($ "%s" (^ inner[1]))` is `<invalid reference>` while `($ "%t" (^ inner[1]))` is
`ref`.

Restoring the location repairs every one of them, because resolution is done at
each read. `(set inner [9 8])` makes `($ "%s" box)` produce `[9]`.

`%v` keeps the `Ref(...)` wrapper, unlike `%s`, `%x` and `%j`, so that
structures containing cycles stay printable.

---

## 8. Arithmetic

Integers are signed 64 bit. Floats are 64 bit IEEE 754.

`div` between two integers always returns an integer, truncated toward zero, the
same as Rust or C. So `div 10 2` is 5 and `div 7 2` is 3, and `div -7 2` is -3.
A division with at least one float returns a float: `div 7.0 2` is 3.5.

Unary `sub` is negation. Unary `div` is the reciprocal, as a float.
`div 0` and `div 0.0` both give `MathError: DivisionByZero`.

`mod` follows C and C++ semantics: truncation toward zero, and the sign of the
result is the sign of the dividend. So `mod -7 2` is -1 and `mod 7 -2` is 1.
`mod` requires integers, `mod 7.0 2` is a `TypeError`.

Division or modulo by zero gives `MathError: DivisionByZero`.

Overflow on `add`, `sub`, `mul`, `pow` and on the negation of the most negative
integer gives `MathError: IntegerOverflow`. Arithmetic is checked, there is no
silent wraparound. A few details:

- `sub` of the most negative integer overflows.
- `pow 2 62` is fine, `pow 2 63` overflows.
- `pow 0 0` is 1 and `pow 0 5` is 0.
- `pow 2 -1` reports `IntegerOverflow`, even though the real cause is a
  negative exponent. `pow 2.0 -1` is 0.5.
- `bit-shl 1 63` overflows, because the result does not fit a signed 64 bit
  integer.

Mixing an integer and a float in an arithmetic operation promotes to float. A
string, bool or null in arithmetic gives a `TypeError`.

The bitwise operations `bit-and`, `bit-or`, `bit-xor`, `bit-not`, `bit-shl` and
`bit-shr` require integers. `bit-and`, `bit-or` and `bit-xor` are variadic
folds with the identities -1, 0 and 0. `bit-not` takes exactly one argument.
`bit-shl` and `bit-shr` take exactly two, and the shift count must be between 0
and 63; anything else gives `MathError: InvalidShiftCount`.

---

## 9. Coercions

| Context | Rule |
| ------- | ---- |
| Arithmetic | int and int is int, float and float is float, mixed is float. `div`: int and int is int truncated toward zero, otherwise float. Other types are a `TypeError`. |
| `eq` / `ne` | Cross-type numeric comparison is allowed, so 5 equals 5.0. Two different non-numeric types are unequal, never an error. |
| `lt` / `gt` / `le` / `ge` | Numeric only, with the same promotion as arithmetic. Non-numeric is a `TypeError`, so `lt "a" "b"` fails. |
| `%s` | Any type. Canonical stringification, with the surprises listed in Section 15.2. |
| `%d` | Requires an integer. |
| `%f` | Integer or float. An integer is rendered exactly, with no float promotion. |
| `%b` `%h` `%o` | Binary, hexadecimal and octal. `%8b`, `%16b`, `%32b`, `%64b` and the matching `%h` forms have a fixed width. |
| `%j` `%t` `%v` | JSON, type name and debug representation. |
| Predicates | See truthiness in Section 1.5. |

---

## 10. eq by type

- Arrays: deep structural equality, order matters.
- Structs: deep structural equality, order does not matter. Structs are compared
  as maps.
- Functions: identity, the same physical closure. Two functions with identical
  code but defined separately are not equal.
- Floats: IEEE 754 comparison with no epsilon. NaN equals nothing, not even
  itself. Signed zeros are equal, so 0.0 equals -0.0. Infinities equal
  themselves.
- References: dereferenced before comparison (Section 7). A composite holding an
  invalid reference compares unequal without raising an error, because it cannot
  equal any value.

`eq` is variadic and true only when all arguments are equal pairwise, not merely
adjacent ones. `ne` is the exact negation of `eq`, true as soon as one pair
differs. With zero or one argument the result is true for `eq`, `lt`, `gt`, `le`
and `ge`, and false for `ne`. With two arguments `ne` is `not eq`.

---

## 11. if, match, and, or, not

### 11.1 if

`if` without an `else` and a false predicate returns `_`.

An `if` branch only opens a scope when the branch is itself a `()`.

### 11.2 match

`match` takes a list of predicate and expression pairs. The first pair whose
predicate is true decides the result. `t` at the head of the last pair always
matches, so it works as a true `else`:

```
(match (eq x 0) "zero" (eq x 1) "one" t "Default")
```

The pairs are scanned from left to right and are only walked as far as needed. A
predicate may be any value and uses truthiness, so `(match f 1 2 3)` returns 3
because the pair `(2 3)` matches. Reaching an argument without a partner is an
`ArityError: match expects predicate/expression pairs`.

If no pair matches and there is no final `t`, the result is
`MatchError: no predicate matched`. This is fail-fast rather than a silent null.
`(match)` with no arguments at all is a `MatchError`.

### 11.3 and

`and` with no arguments is `t`. Otherwise it evaluates left to right and returns
the first falsy value, or the last value when everything is true.

```
(and)            ; t
(and 1 2)        ; 2
(and 0 1)        ; 0
(and 1 f 2)      ; f, the rest is not evaluated
(and [] 1)       ; 1
```

### 11.4 or

`or` with no arguments is `f`. Otherwise it evaluates left to right and returns
the first truthy value, or the last value when everything is false.

```
(or)             ; f
(or _ 2)         ; 2
(or f f)         ; f
(or "" 1)        ; "", the empty string is true
```

### 11.5 not

`not` takes exactly one argument and returns a bool that is the negation of
truthiness. `not 1` is `f` and `not "a"` is `f`. `not` with no argument is an
`ArityError`.

### 11.6 expect

```
(expect actual expected)
(expect actual expected message)
```

Returns `t` when the two values are equal by the rules of Section 10, and raises
`ExpectationError` otherwise. Comparison is cross-type numeric, so 1 and 1.0 are
equal. Functions compare by identity.

With two arguments the message is
`expectation failed: expected <value>, got <value>`. With three arguments the
message starts with the given string. The third argument must be a string.
Exactly two or three arguments are accepted, anything else is an `ArityError:
expect expects 2 or 3 arguments, got N`. Both values in the message use the
debug form of Section 15.9.

```
(expect 1 2)                 ; ExpectationError: expectation failed: expected Int(2), got Int(1)
(expect [1] [2] "differ")    ; ExpectationError: differ: expected Array([Int(2)]), got Array([Int(1)])
```

---

## 12. loop, break, continue

The only target of `break` is a `loop`.

`break value` makes the whole loop evaluate to `value`. A bare `break` makes the
loop evaluate to `_`.

A `break` inside a `match` nested in a loop still leaves the loop. The `match` is
traversed, it is not a break target. `break` in a `match` with no enclosing loop
is a `BreakOutsideLoop` error.

A `break` inside a function that was called from a loop is **not** limited to
leaving that function. It is a `BreakOutsideLoop` error, because the function
body is not in the loop. The same is true of `continue`, which gives
`ContinueOutsideLoop`.

```
(let s 0)
(let h (fn () (break "b")))
(loop (set s (add s 1)) (h))
; BreakOutsideLoop
```

`break` accepts zero or one argument, `ArityError: break expects zero or one
argument` otherwise. `continue` accepts none, `ArityError: continue expects 0
arguments, got 1` otherwise. Outside a loop both report the loop error first, so
`(break 1 2)` outside a loop is a `BreakOutsideLoop` and not an arity error.

A `loop` with an empty body never ends. There is no iteration guard.

```
(let s 0) (loop (set s (add s 1)) (if (eq s 3) (break s)))   ; s is 3
```

---

## 13. IO

The `io` module is loaded with `(use "io")`.

| Function | `arity` | `type` | `return` |
| -------- | ------- | ------ | -------- |
| `io.open uri` | 1 | `["string"]` | `["int"]` |
| `io.read fd` | 1 | `["int"]` | `["string"]` |
| `io.write fd text` | 2 | `["int", "string"]` | `["int"]` |
| `io.close fd` | 1 | `["int"]` | `["bool"]` |

The `type` check runs before the function body, so a wrong argument type is
reported as `TypeError: module function argument 1 expects string, got int` even
when the function would have rejected it too.

### 13.1 open

```
(io.open "file:PATH?mode=MODE")
```

`MODE` is one of `r`, `r+`, `w`, `w+`, `a`, `a+`. The call returns an integer
descriptor.

The URI must start with `file:`. It must contain a `?`. The path must be
non-empty and the query must be non-empty. The query must contain a `mode=`
parameter; other parameters are ignored and `mode` may appear anywhere in the
query. The path is used literally, there is no percent-decoding.

| Mode | Truncates | Creates | Readable | Writable | Writes at |
| ---- | --------- | ------- | -------- | -------- | --------- |
| `r`  | no        | no      | yes      | no       | n/a |
| `r+` | no        | no      | yes      | yes      | current file position |
| `w`  | yes       | yes     | no       | yes      | current file position |
| `w+` | yes       | yes     | yes      | yes      | current file position |
| `a`  | no        | yes     | no       | yes      | end of file |
| `a+` | no        | yes     | yes      | yes      | end of file |

`r+` does not create the file, so opening a missing file with it is an `IOError`
from the operating system.

Descriptors 0, 1 and 2 are stdin, stdout and stderr. Files receive descriptors
starting at 3.

Errors: a URI that is not a `file:` URI, a missing `?`, an empty path, an empty
query, a missing `mode`, and an unsupported mode are all `IOError`.

### 13.2 read

```
(io.read fd)
```

Reads one line and returns it as a string, without its line terminator. At end of
file it returns `_`.

Both LF and CRLF terminators are removed, as a unit. Reads are buffered and the
buffer belongs to the opened descriptor, so data prefetched by one read stays
available to the next read on the same descriptor.

A descriptor that is not readable, and a descriptor that is not open, give an
`IOError`. `io.read 1` and `io.read 2` are errors. `io.read 0` reads stdin.
Input that is not valid UTF-8 gives `IOError: input is not valid UTF-8`.

### 13.3 write

```
(io.write fd text)
```

Writes the string and returns the number of bytes written, which is the byte
length of the text, not the number of characters.

A descriptor that is not writable, and one that is not open, give an `IOError`.
`io.write 0` is an error. Writes to descriptors 1 and 2 are flushed immediately.

### 13.4 close

```
(io.close fd)
```

Closes the descriptor and returns `t` if it was open, `f` otherwise. Closing a
descriptor twice returns `f`.

Descriptors 1 and 2 can be closed too. After `io.close 1` every later write to
descriptor 1 fails with `IOError: invalid file descriptor 1`.

### 13.5 Mixed modes

For `r+`, `w+` and `a+`, reads and writes share the same file and the same file
position. A write goes to the current file position. Because reading is
buffered, a read has already advanced that position past what it consumed, so an
interleaved write lands after the prefetched block. `a+` is the exception: it
always writes at the end of the file.

A write-only descriptor (`w`, `a`) is not readable. A read-only descriptor (`r`)
is not writable.

---

## 14. HTTP

The `http` module is loaded with `(use "http")`.

| Function | `arity` | `type` | `return` |
| -------- | ------- | ------ | -------- |
| `http.get url` | 1 | `["string"]` | `_` |
| `http.head url` | 1 | `["string"]` | `_` |
| `http.delete url` | 1 | `["string"]` | `_` |
| `http.post url body` | 2 | `["string", "any"]` | `_` |
| `http.put url body` | 2 | `["string", "any"]` | `_` |
| `http.patch url body` | 2 | `["string", "any"]` | `_` |

`return` is `_` because the response type depends on the `Content-Type` header.

Every call performs one synchronous HTTP request with the matching method. The
request carries `Accept: application/json`. A request with a body also carries
`Content-Type: application/json`.

The body is serialised to JSON from the Lisp value, using the `%j` rules. A
string therefore becomes a JSON string, `"hello"`, and a struct becomes a JSON
object. A value that cannot be encoded, such as a function or a non-finite float,
gives a `FormatError`.

A `HEAD` request always returns the empty string, whatever the response body.

For the other methods, a `Content-Type` of `application/json`, ignoring case and
ignoring any parameters after the first `;`, or any type whose subtype ends in
`+json`, the RFC 6839 structured syntax suffix, such as
`application/problem+json`, triggers decoding into a Lisp value. Every other type
returns the body as a string.

A decoded JSON object becomes a struct whose keys are in **alphabetical** order,
not in document order, because the JSON map is a sorted map. Nested objects are
sorted the same way. JSON `null` becomes `_`, `true` and `false` become `t` and
`f`, a number that fits an i64 becomes an integer, and any other number becomes
a float, so `12345678901234567890` comes back as the float
`12345678901234567000.0`. A JSON array becomes an array.

```
(http.get "http://host/json")
; from the body {"zebra":1 "apple":{"y":2,"x":3} "nil":null}
; gives {apple:{x:3 y:2} nil:_ zebra:1}
```

Descriptor arity and type errors give `ArityError` and `TypeError`. A URL that
cannot be parsed or a transport failure gives
`HTTPError: HTTP request failed: ...`. An HTTP status of 4xx or 5xx gives
`HTTPError: HTTP request failed with status 404` and the response body is not
returned. A body that cannot be read gives
`HTTPError: failed to read HTTP response: ...` and a malformed JSON body gives
`HTTPError: invalid JSON response: ...`.

---

## 15. Interpolation

```
($ format arguments...)
```

The first argument must be a string, otherwise `TypeError: expected string`. With
no argument at all the result is `ArityError: $ expects format string`.

The remaining arguments are consumed in order by the specifiers. The number of
specifiers that consume an argument must match the number of arguments exactly,
otherwise the result is `FormatError: FormatArityError`. Passing an argument
that no specifier consumes is an error.

The result of `$` is always a string.

### 15.1 Specifiers

| Specifier | Accepts | Produces |
| --------- | ------- | -------- |
| `%s` | any | canonical stringification |
| `%q` | string | a quoted Lisp string literal, reusable by the reader |
| `%x` | any except functions | canonical Lisp source, reusable by the reader |
| `%d` | integer | signed decimal |
| `%b` | integer | binary, no imposed width |
| `%h` | integer | lowercase hexadecimal, no `0x` prefix |
| `%o` | integer | octal, no `0o` prefix |
| `%f` | integer or float | decimal |
| `%j` | JSON-serialisable value | compact JSON |
| `%t` | any | the type name |
| `%v` | any | the debug representation |
| `%%` | nothing | a literal `%` |
| `%~` | two strings | nothing by itself, it sets the current matches |
| `%M.C` | nothing | capture C of match M |
| `%N` | nothing | same as `%1.N` |

A type that does not fit `%d`, `%b`, `%h`, `%o`, `%f` or `%j` gives a
`FormatError` with the sub-message `FormatTypeError`.

### 15.2 s

`%s` accepts any type and produces the canonical stringification.

| Value | Result |
| ----- | ------ |
| a string at the root | the characters themselves, unquoted |
| a string nested in an array or struct | quoted, JSON escaped |
| a struct key | never quoted |
| `t` / `f` | `true` / `false` |
| `_` | the empty string |
| an integer | its decimal form |
| a float | its numeric form |
| a function | `<fn>` |
| a native function | `<native fn>` |

So `($ "%s" "a")` is `a`, `($ "%s" ["a"])` is `["a"]` and `($ "%s" {a: "b"})` is
`{a:"b"}`.

Array elements are separated by one space, and so are struct fields:
`($ "%s" [1 2])` is `[1 2]` and `($ "%s" {a: 1 b: 2})` is `{a:1 b:2}`.

Booleans are rendered as `true` and `false`, and null as nothing at all, so the
output of `%s` is not Lisp source and does not read back. Use `%x` for that.

References are dereferenced. An invalid reference renders as `null` in a nested
position and `<invalid reference>` at the root of `%s`. The literal `null` only
ever appears for an invalid reference, never for `_`.

Escaping depends on the position. A string at the root is emitted raw, while a
string nested in an array or a struct is escaped the way JSON escapes, so a
control character below 0x1F becomes `\u00XX` there. Given a string holding the
bytes `61 01 62 1f 63`:

```
($ "%s" s)     ; the raw characters, nothing is escaped
($ "%s" [s])   ; ["a\u0001b\u001fc"]
($ "%j" s)     ; "a\u0001b\u001fc"
($ "%q" s)     ; the raw characters
($ "%x" [s])   ; the raw characters
($ "%v" s)     ; Str("a\u{1}b\u{1f}c")
```

`%q` and `%x` only apply the five escapes of Sections 15.3 and 15.4, so they
pass a control character through unchanged. `%v` uses Rust's own debug
escaping, which writes `\u{1}` rather than `\u0001`.

The reader has no `\u` escape. It drops the backslash of an unknown escape and
keeps the letter, so `\u0001` reads back as `u0001`. A string holding a control
character therefore survives `%q` and `%x` but not `%s` on a nested value. See
Appendix C.

### 15.3 q

`%q` requires a string and produces a Lisp string literal between double quotes,
reusable by the reader:

| Character | Emitted as |
| --------- | ---------- |
| `"` | `\"` |
| `\` | `\\` |
| line feed | `\n` |
| carriage return | `\r` |
| tab | `\t` |
| anything else | itself |

The result re-parses as the same string. References are dereferenced.

### 15.4 x

`%x` serialises any value into canonical Lisp source, reusable by the reader.

Nested strings use the rules above. Booleans become `t` and `f`. `_` stays `_`.
Floats use their numeric representation, `NaN`, `Inf` and `-Inf` included. Arrays
become `[a b c]`. Structs become `{key: value ...}` in insertion order. Functions
have no source representation and are refused.

The result re-parses as a value equal to the original. References are
dereferenced, and an invalid reference propagates a type error.

A float is written in plain decimal, never in exponent notation, so
`($ "%x" 100000000000000000000.0)` is `100000000000000000000.0` and reads back
as the same value. Rust's exponent form is expanded before printing.

### 15.5 d, b, h, o

`%d` requires an integer and produces its signed decimal form.

`%b` requires an integer and produces its binary form with no imposed width.

`%8b`, `%16b`, `%32b` and `%64b` require an integer and produce a fixed width
binary form. A negative value uses its two's complement representation truncated
to the requested width. `%8b` of 256 is `00000000`. Only the widths 8, 16, 32
and 64 are accepted, so `%0b` and `%7b` are errors.

`%h` requires an integer and produces lowercase hexadecimal with no `0x` prefix.
`%8h`, `%16h`, `%32h` and `%64h` produce a fixed width hexadecimal form of 2, 4,
8 or 16 digits. A negative value uses two's complement truncated to the
requested width.

`%o` requires an integer and produces its octal form with no `0o` prefix. There
is no width form, so `%8o` is not an octal width, it is parsed as a capture
selector. A negative integer prints its full 64 bit two's complement in octal,
22 digits, with no minus sign, so `%o` of -1 is `1777777777777777777777`.

### 15.6 f

`%f` accepts an integer or a float. An integer is rendered exactly in decimal,
with no conversion through a float, so there is no loss of precision for large
integers. A float is rendered with its numeric representation.

There is no width and no precision form.

### 15.7 j

`%j` accepts the values that can be serialised as JSON and produces a compact
JSON string. Struct keys are quoted JSON strings.

- `_` becomes `null`.
- `t` and `f` become `true` and `false`.
- A struct becomes a quoted-key object.
- A float with an integral value is written without a fraction, so 1.0 becomes
  `1`.
- Functions are not serialisable and are refused.
- The floats `NaN`, `Inf` and `-Inf` are refused.

References are dereferenced. An invalid reference propagates a type error.

### 15.8 t

`%t` accepts any type and produces its type name:

```
null  bool  int  float  string  array  struct  function  ref
```

A native function reports `function`. A float of 0.0 reports `float`, not the
truthiness.

### 15.9 v

`%v` accepts any type and produces its structural debug representation:

```
Int(1)  Float(1.5)  Str("a")  Bool(true)  Null
Array([Int(1)])  Struct({name: Int(1)})
Function(b, c)  NativeFunction(io.write)  Ref(Int(1))
```

Struct keys follow Lisp syntax, `name: value`, without quotes. A function shows
its parameter list, not its binding name. A reference appears as `Ref(...)` with
its content rendered by dereferencing the location, and an invalid reference
becomes `Ref(<invalid>)`. The wrapper is kept, unlike `%s`, `%x` and `%j`, so
that structures containing cycles stay printable.

`%v` is a Rust debug rendering, so a float infinity is lowercase:
`($ "%v" Inf)` is `Float(inf)`, while `%s` and `%x` use `Inf`. A string is shown
with Rust's own debug escaping, which is close to JSON but not identical for
rare characters.

### 15.10 Capture selectors

`%~` requires two arguments: a regex string, then a string to match. The
resulting matches become the current match list. `%~` emits nothing by itself.

`%M.C` emits capture C of the M-th match. C is 1 for the whole match, 2 for the
first capturing group, and so on. `%N` is a shortcut for `%1.N`, so `%1` is the
whole first match and `%2` is the first capturing group. These selectors consume
no argument.

| Situation | Result |
| --------- | ------ |
| no match at all | the single character `f` is emitted |
| an optional group that did not participate | `_` |
| a selector with no preceding `%~` | `FormatError` |
| a zero match index, such as `%0` or `%0.1` | `FormatError` |
| a match index or capture index out of range | `FormatError` |
| a capture index that is not a number, such as `%1.x` | `FormatError` |
| a selector such as `%x` that is not a number at all | the whole `$` call is a `FormatError: FormatArityError` |

The selector is checked in this order: a missing `%~` is reported first, then a
match index below 1, then a capture index below 1, then an out of range match
index, then an out of range capture index. So `($ "%0.1" "a" "a")` complains
about the missing `%~`, while `($ "%~%0.1" "a" "a")` complains about the match
index.

A format string may contain several `%~` sections, but each one consumes two
more arguments, and each one replaces the current match list for the selectors
that follow it. A bare `%~` with its two arguments emits nothing.

### 15.11 Errors

| Situation | Printed text |
| --------- | ------------ |
| no argument at all | `ArityError: $ expects format string` |
| the format is not a string | `TypeError: expected string` |
| a trailing `%` | `FormatError: trailing %` |
| an unknown specifier | `FormatError: unknown specifier %z` |
| a selector with no preceding `%~` | `FormatError: FormatError: capture selector without preceding %~` |
| a match index below 1 | `FormatError: FormatError: match index must be at least 1` |
| a capture index below 1 | `FormatError: FormatError: capture index must be at least 1` |
| a match index out of range | `FormatError: FormatError: match index N out of range` |
| a capture index out of range | `FormatError: FormatError: capture index N out of range` |
| a capture index that is not a number | `FormatError: invalid capture index` |
| a bad binary width | `FormatError: FormatTypeError: %7b supports widths 8, 16, 32, or 64` |
| a bad hexadecimal width | `FormatError: FormatTypeError: %9h supports widths 8, 16, 32, or 64` |
| an integer expected by `%d` `%b` `%h` `%o` | `FormatError: FormatTypeError: %d expects integer` |
| a number expected by `%f` | `FormatError: FormatTypeError: %f expects number` |
| a string expected by `%q` | `FormatError: FormatTypeError: %q expects string` |
| strings expected by `%~` | `FormatError: FormatTypeError: %~ expects string` |
| a function given to `%x` or `%j` | `FormatError: FormatTypeError: %j cannot encode function` |
| a non-finite float given to `%j` | `FormatError: FormatTypeError: %j cannot encode non-finite float` |
| the argument count does not match | `FormatError: FormatArityError` |
| an invalid regex | `InvalidRegex: invalid regex: ...` |

A `FormatError` carries a sub-message. The printed text is the category prefix
followed by that sub-message. Seven of the places that build a sub-message
already begin with `FormatError: `, so the prefix appears twice. Those seven are
the two out of range messages, and, in the selector scanner, the missing `%~`
message and the capture-index-below-1 message, each of which is written twice
because the scanner has two selector branches. See Appendix C.

---

## 16. Regex

Regular expression matching is only available through the interpolation
specifier `%~` (Section 15). The selectors `%M.C` and `%N` then emit the whole
match and the capturing groups.

The regexes use the Rust `regex` engine and operate on UTF-8 strings. For each
match, cell 1 is the whole match and the capturing groups follow in order, so
cell 2 is the first group, cell 3 is the second, and so on.

**Regexes operate on Unicode scalars, not on grapheme clusters.** A match or a
capture can start or end in the middle of a grapheme. On a string made of the
letter `e` followed by the combining acute accent U+0301, the group `(.)` and
the pattern `.` capture only the `e`, while string indexing returns the whole
cluster.

An optional group that did not participate produces `_`. A string with no match
at all produces the character `f` for any selector.

An invalid regex gives `InvalidRegex` with the engine's diagnostic, prefixed by
`invalid regex: `, for example
`InvalidRegex: invalid regex: regex parse error: ... unclosed group`. A
non-string argument gives `FormatError: FormatTypeError: %~ expects string`.

### 16.1 Options

The regex argument follows the form `[options]~pattern`. The option letters
`g`, `m`, `i`, `s`, `x`, `U`, `u`, `R` written before the `~` form the options
block, which replaces the default. Everything after the `~` is the pattern.
Without a block, the default options are `gmu`.

| Option | Meaning |
| ------ | ------- |
| `g` | find all matches. This is the default. Without `g` only the first match is looked for. |
| `m` | line anchors |
| `i` | case insensitive |
| `s` | the dot matches line breaks |
| `x` | whitespace and comments are ignored |
| `U` | swap greediness |
| `u` | unicode. Already on in the engine, so this letter changes nothing. |
| `R` | treat CRLF as a line ending |

`g` is what makes several matches available. It is on by default, so a second
match is reachable out of the box. Drop it from the options block and only the
first match exists, which turns `%2.1` into an out of range error. A bare `%N` is
`%1.N`, so it is a capture of the first match and does not need `g`:

```
($ "%~%1.1" "(a)(b)" "ab")   ; ab, the whole match
($ "%~%1.2" "(a)(b)" "ab")   ; a,  the first group
($ "%~%1.3" "(a)(b)" "ab")   ; b,  the second group
($ "%~%2"   "(a)(b)" "ab")   ; a,  the same as %1.2
($ "%~%2.1" "(a)"   "aa")    ; a,  the second match, thanks to the default g
($ "%~%2.1" "m~(a)" "aa")    ; FormatError: match index 2 out of range
```

A pattern that starts with option letters followed by `~` is interpreted as an
options block. This is a known limitation with no escape. The block must have at
least one letter, so `"~a"` is a pattern that begins with a tilde, not an
options block.

---

## 17. eval

```
(eval source)
```

`source` must be a string, otherwise `TypeError: expected string`. The call
evaluates `source` as Lisp code and returns the value of the last form.

The string is lexed and parsed as a program (Section 3), then each form is
evaluated in the caller's current environment. The visible bindings are
accessible, and `let`, `set` and `use` all work in the evaluated code. A `let`
inside the evaluated code binds in the caller's scope, so the name is visible
after the `eval` returns.

```
(eval "1 2 3")          ; 3, the value of the last form
(eval "")               ; _, an empty program
(eval "; a comment")    ; _
(eval "1)")             ; ParseError: unexpected end
```

```
(let q 1)
(eval "(let z 2)")
z    ; 2, the evaluated let bound in this scope
```

```
(let q 1)
(eval "(set q 9)")
q    ; 9, the evaluated set reached this binding
```

The evaluated code inherits the loop and match depth of the caller, so `break`
and `continue` propagate to the enclosing loop just like code at the same level.

```
(let s 0)
(loop (set s (add s 1)) (eval "(break 1)") (break 2))
; the loop evaluates to 1
```

Inner parse and execution errors propagate normally. Their diagnostic points at
the host program's `(eval ...)` call, at the first character of that call, so a
parse error inside the string is reported at the `eval` site rather than inside
the string:

```
prog.lisp:1:1: NameError: nope
(eval "(nope)")
^
```

---

## 18. Errors

The error categories are:

```
ParseError  NameError  ModuleError  TypeError  ArityError  MathError
IOError  HTTPError  FormatError  InvalidRegex  DuplicateKeyError
DuplicateBindingError  MatchError  ExpectationError  RecursionError
ContinueOutsideLoop  BreakOutsideLoop  Interrupted  Quit
```

Format errors carry a sub-message. The two are `FormatArityError` for an
argument count mismatch and `FormatTypeError` for a type or selector problem.
Some of them already start with `FormatError: `, so the prefix appears twice.
See Section 15.11.

There is no try and catch. The evaluation is fail-fast: the first error stops
the program, the message goes to stderr, and the exit code is 1. A successful run
exits with 0.

Parse and runtime errors print the file, the line, the column, the source line
and a `^` caret under the column:

```
/tmp/tr5.lisp:2:9: TypeError: expected number
(fn (b) (add a "x") 1)
        ^
```

The column is counted in characters, not bytes, so a multi-byte character on the
same line counts as one.

Errors that cross user functions also print a call trace, one line per frame,
innermost first:

```
call trace:
  a at prog.lisp:3:14
  b at prog.lisp:7:1
  … 2009 more frame(s) omitted
```

The two lines are indented by two spaces. The frame name is the `let` name of
the function, or `<anonymous function>` when the function was not bound to a
name. At most 40 frames are listed, then the count of the remaining ones is
printed on its own line.

Note that the line number in a diagnostic does not account for a shebang line
that was removed before parsing, while the REPL's own line numbers do. See
Section 21 and Appendix C.

On Unix, Ctrl-C is turned into `Interrupted` and points at the expression
currently being evaluated.

`Quit` is raised only by the REPL's `:q` command.

---

## 19. Structs

- Insertion order is preserved.
- A duplicate key at construction is a `DuplicateKeyError`, never a silent
  last-one-wins.
- A key is a single unquoted name, as in Section 2.1. `{1: 2}` and `{"a": 1}` are
  `ParseError`.
- In operator position, only a complete descriptor, meaning `_` and `spec`, is
  callable. The call unwraps the value of `_`, checks that it is callable, in
  exactly one level (Section 2.9). Every other evaluation, including field
  access and interpolation, keeps the full struct.

---

## 20. REPL

`(repl)` is a special form that suspends the program at the exact call site and
opens an interactive session. Each line is evaluated as a Lisp program in a
throwaway child scope, with the same loop and match depths as the call.

`(repl "label")` names the session.

### 20.1 The prompt

The prompt is contextual:

| Situation | Prompt |
| --------- | ------ |
| at the root with no source context | `repl> ` |
| inside a program | `file:line> ` |
| with a label | `label@file:line> ` |
| a `repl` typed inside a session line | `<repl>:1> ` |

The line is the execution point, that is the current `(repl)`, in the real
program source, even if a shebang line was removed before parsing.

The bare `repl> ` form cannot normally be reached, because loading a program
always registers a source context.

### 20.2 Lines

Anything that does not start with `:` is evaluated. The value of the last form is
echoed on the session terminal, unless it is `_`, in which case nothing is
echoed. The echo uses Lisp source form, so a string is shown with quotes and
escapes, an array as `[1 2]`, a struct as `{a:1}`, a function as `<fn>` and a
native function as `<native fn>`:

```
> "a string"     ; "a string"
> [1 2]          ; [1 2]
> {a: 1}         ; {a:1}
> (fn (x) x)     ; <fn>
> _              ; nothing
```

The echo goes to the terminal, not to the program's stdout, so redirecting
stdout does not capture it. Blank lines are ignored.

### 20.3 Commands

A line starting with `:` is a command.

| Command | Effect |
| ------- | ------ |
| `:c` `:continue` | resume the program |
| `:q` `:quit` | abort the run with a `Quit` error |
| `:h` `:help` | list the commands |
| `:l` `:list [N]` | show the source around the execution point |
| `:i` `:inspect [name]` | show the bindings |
| `:bt` `:backtrace` | show the call stack |

End of file on the session input ends the session like `:c`.

As in the evaluator, the resume, quit and help commands take no argument. The
whole token must match, so `:c 1` is reported as an unknown command:

```
repl: unknown command `:c 1` (try :h)
```

`:h` prints one long line, wrapped here for reading:

```
commands: :c/:continue resume, :q/:quit abort, :l/:list [N] show source around
the execution point, :i/:inspect [name] show the bindings, :bt/:backtrace show
the call stack, :h/:help this help; anything else is evaluated as Lisp
```

### 20.4 list

```
:l          ; the 3 lines on each side of the execution point
:l N        ; a window of N lines on each side
:l 0        ; the current line only
```

The header is `@ file:line (execution point)`. Each line below it is written as
a space, then `>` for the execution point or a space otherwise, then the line
number right-aligned, then two spaces, then the trimmed source text.

```
@ prog.lisp:2 (execution point)
   1  (use "io")
 > 2  (repl)
   3  (io.write 1 "END")
```

The window is clamped at the two ends of the file.

A negative or non-numeric count is refused:

```
repl: :l expects a non-negative count, got `-1`
```

### 20.5 inspect

```
:i          ; list the effective bindings
:i name     ; describe one binding
```

The listing shows each name once, the innermost binding winning, in alphabetical
order, with the value cut at 60 characters and a `…` appended. A name is marked
with ` *` when it also has an outer binding, that is when it is shadowed. The
header is `N bindings across M scopes`, with an `s` added to both words when the
count is not 1. At most 200 rows are listed, then `… and N more bindings`.

```
> :i
2 bindings across 2 scopes
io = Struct({open: Struct({_: NativeFunction(io.open), spec: Stru…
n = 2 *
```

`name` is a plain binding name, not a path, so `:i io.write` does not work. The
command walks the scope chain and describes the first binding it finds. The
scope is a number, counted from the session outwards, with a word after it:
`session` for 0, `enclosing` for 1, and `outer` for anything deeper. Each
successive outer binding is then listed as `outer NAME = VALUE @ scope N`.

```
> :i n
n = 2   int   scope 0 (session)
  outer n = 1 @ scope 1

> :i hlp
hlp = (fn hlp (y))   function   scope 2 (outer)
  def: prog.lisp:3
```

A function value adds a `def:` line with `label:line`. The three texts are the
resolved position, `defined in this session (repl)`, and
`defined outside the current source`. A native function gets no `def:` line.

An unknown name gives `repl: no binding named \`name\``.

### 20.6 backtrace

```
:bt
```

Shows the live call stack, innermost first, as `name at file:line:col   (text)`.

```
> :bt
backtrace (1 frame)
  g at prog.lisp:4:1   ((g 3))
```

It is empty when the `(repl)` is at the top level rather than inside a function,
and the empty case is reported as such:

```
repl: backtrace is empty ((repl) is at the top level, not inside a function)
```

### 20.7 Source trails

The source being evaluated, meaning the program file, each `(eval ...)` and each
session line, is registered with its label, its text and its line offset. That
is what lets `:l`, `:i` and `:bt` resolve positions. A function defined in a
session is reported as such.

### 20.8 The child scope

- `let` binds only in the session. There is no trace of it after `:c`.
- `set` and reads reach the program's live state.
- An error on a line is displayed as `<repl>:1:col: message` with the line and a
  caret, and the session continues. It never propagates to the program. A parse
  error points at the offending column, a runtime error at the failing call.
- `(break)` and `(continue)` typed in a session propagate and act on the
  enclosing loop.
- A nested `(repl)` works, recursively, and its `:q` aborts the whole run.

A `:q` at any depth ends the run with exit code 1. The `Quit` error is reported
at the `(repl)` call itself, with the source line and a caret:

```
prog.lisp:2:1: Quit: repl: aborted by user (:quit)
(repl)
^
```

The message goes to the terminal, not to the program's stderr, so it does not
appear in a redirect.

### 20.9 The terminal

The session reads and writes on the process's controlling terminal, `/dev/tty`.
The program's stdin is never consumed by the session, even when the program was
loaded from stdin, so `io.read 0` keeps reading the original stdin.

Without a controlling terminal, `(repl)` fails. There is no silent fallback to
stdin.

```
prog.lisp:2:1: IOError: repl: cannot open the controlling terminal (/dev/tty): Device not configured (os error 6)
(repl)
^
```

### 20.10 Ctrl-C

During a session, Ctrl-C cancels the current line, prints
`repl: interrupted (:c continues, :q quits)`, and the session continues.

The Ctrl-C disposition from before the session is restored on exit, so a Ctrl-C
after `:c` still aborts the run as before. Outside a session, Ctrl-C keeps
aborting the run with `Interrupted`.

---

## 21. Implementation

The interpreter is a tree-walking evaluator in Rust. The pipeline is lex, parse
to an AST, then recursive evaluation.

### 21.1 Command line

```
small-lisp [file]
```

With a file argument, that file is the program. Without one, the program is read
from stdin. Only the first argument is used; any further arguments are ignored,
and there is no `--help` or `--version` flag. A path that is treated as a flag
simply does not exist as a file.

The program name shown in diagnostics is the file path, or `<stdin>`.

A missing file and a directory are `IOError`. The exit code is 1 for any error
and 0 for a successful run.

### 21.2 Shebang

A leading `#!` line is removed before parsing, so a script can be made
executable. The line number in a diagnostic does not account for the removed
line, so an error on the second physical line is reported as line 1. The REPL's
line numbers do account for it.

### 21.3 Source encoding

CRLF line endings in the source are accepted and do not shift the reported
columns.

A UTF-8 byte order mark is not stripped. The mark is not whitespace, so the
lexer reads it as the first character of a name and the name run stops at the
following bracket. The result is a `NameError` whose message is the single
invisible mark character, reported on the first line at column 1.

An empty file runs and exits with 0.

### 21.4 Limits

| Limit | Value |
| ----- | ----- |
| Maximum call depth | 2048 |
| Interpreter thread stack | 256 MiB |
| Call trace frames printed | 40 |

---

## Appendix A. Built-in functions

| Call | Arity | Result |
| ---- | ----- | ------ |
| `(add ...)` | 0 or more | Sum. Identity 0. |
| `(sub a ...)` | 1 or more | Left fold. Unary is negation. |
| `(mul ...)` | 0 or more | Product. Identity 1. |
| `(div a ...)` | 1 or more | Left fold. Unary is the reciprocal as a float. `div 0` is DivisionByZero. |
| `(mod a b)` | exactly 2 | Integers only. Sign of the dividend. |
| `(pow a b)` | exactly 2 | Integer path is checked. Float path is a float power. |
| `(eq ...)` | 0 or more | True when all arguments are pairwise equal. |
| `(ne ...)` | 0 or more | Exact negation of `eq`. |
| `(lt ...)` | 0 or more | Numeric chained less-than. |
| `(gt ...)` | 0 or more | Numeric chained greater-than. |
| `(le ...)` | 0 or more | Numeric chained less-or-equal. |
| `(ge ...)` | 0 or more | Numeric chained greater-or-equal. |
| `(bit-and ...)` | 0 or more | Variadic. Identity -1. |
| `(bit-or ...)` | 0 or more | Variadic. Identity 0. |
| `(bit-xor ...)` | 0 or more | Variadic. Identity 0. |
| `(bit-not a)` | exactly 1 | |
| `(bit-shl a n)` | exactly 2 | `n` must be 0 to 63. |
| `(bit-shr a n)` | exactly 2 | `n` must be 0 to 63. |
| `($ fmt ...)` | 1 or more | Section 15. |
| `(and ...)` | 0 or more | Section 11.3. |
| `(or ...)` | 0 or more | Section 11.4. |
| `(not a)` | exactly 1 | Section 11.5. |
| `(expect a b ...)` | 2 or 3 | Section 11.6. |
| `(eval src)` | exactly 1 | Section 17. |
| `(repl ...)` | 0 or 1 | Section 20. |
| `(use name)` | exactly 1 | Section 2.7. |

With zero or one argument, `eq`, `lt`, `gt`, `le` and `ge` are true, and `ne` is
false.

The arity messages are uniform: `NAME expects N arguments, got M`, except for
`sub` and `div`, which say `NAME expects at least 1 argument, got 0`, and for
`repl`, which says `repl expects zero or one argument` without a count. A `repl`
label must be a string, so `(repl 5)` is a `TypeError: expected string`.

---

## Appendix B. Format specifiers

Quick reference. Section 15 is normative.

| Specifier | Example | Result |
| --------- | ------- | ------ |
| `%s` | `($ "%s" "a")` | `a` |
| `%s` | `($ "%s" [1 "a"])` | `[1 "a"]` |
| `%s` | `($ "%s" {a: 1})` | `{a:1}` |
| `%s` | `($ "%s" {a: 1 b: 2})` | `{a:1 b:2}` |
| `%s` | `($ "%s" [t f _])` | `[true false ]` |
| `%s` | `($ "%s" (fn (x) x))` | `<fn>` |
| `%s` | `($ "%s" str.upper._)` | `<native fn>` |
| `%q` | `($ "%q" "hi\n")` | `"hi\n"` |
| `%q` | `($ "%q" "it's")` | `"it's"` |
| `%q` | `($ "%q" "a\"b")` | `"a\"b"` |
| `%q` | `($ "%q" "a\\b")` | `"a\\b"` |
| `%x` | `($ "%x" [1 t _])` | `[1 t _]` |
| `%x` | `($ "%x" {a: 1})` | `{a:1}` |
| `%x` | `($ "%x" "a\nb")` | `"a\nb"` |
| `%x` | `($ "%x" Inf)` | `Inf` |
| `%d` | `($ "%d" 42)` | `42` |
| `%b` | `($ "%b" 5)` | `101` |
| `%8b` | `($ "%8b" 256)` | `00000000` |
| `%8b` | `($ "%8b" -1)` | `11111111` |
| `%16b` | `($ "%16b" -1)` | `1111111111111111` |
| `%32b` | `($ "%32b" -1)` | `11111111111111111111111111111111` |
| `%h` | `($ "%h" 255)` | `ff` |
| `%16h` | `($ "%16h" -1)` | `ffff` |
| `%32h` | `($ "%32h" -1)` | `ffffffff` |
| `%64h` | `($ "%64h" -1)` | `ffffffffffffffff` |
| `%o` | `($ "%o" 8)` | `10` |
| `%o` | `($ "%o" -1)` | `1777777777777777777777` |
| `%f` | `($ "%f" 1.5)` | `1.5` |
| `%f` | `($ "%f" 9007199254740993)` | `9007199254740993` |
| `%f` | `($ "%f" 9223372036854775807)` | `9223372036854775807` |
| `%f` | `($ "%f" NaN)` | `NaN` |
| `%j` | `($ "%j" {a: 1})` | `{"a":1}` |
| `%j` | `($ "%j" [1 "a"])` | `[1,"a"]` |
| `%j` | `($ "%j" 1.0)` | `1` |
| `%t` | `($ "%t" [1])` | `array` |
| `%t` | `($ "%t" 0.0)` | `float` |
| `%t` | `($ "%t" (^ a))` | `ref` |
| `%v` | `($ "%v" [1])` | `Array([Int(1)])` |
| `%v` | `($ "%v" {a: 1})` | `Struct({a: Int(1)})` |
| `%v` | `($ "%v" (^ a[1]))` | `Ref(Int(1))` |
| `%%` | `($ "100%%")` | `100%` |
| `%~%1.1` | `($ "%~%1.1" "(a)(b)" "ab")` | `ab` |
| `%~%1.2` | `($ "%~%1.2" "(a)(b)" "ab")` | `a` |
| `%~%2` | `($ "%~%2" "(a)(b)" "ab")` | `a` |
| `%~` with no match | `($ "[%~%1.1]" "zz" "ab")` | `[f]` |

---

## Appendix C. Differences from spec.txt

This appendix records where this document contradicts `spec.txt`, so the change
is auditable. Each item was verified against the interpreter.

### Corrected claims

1. `break` in a function called from a loop. spec.txt section 12 says it only
   leaves the function. In fact it raises `BreakOutsideLoop`, and `continue`
   raises `ContinueOutsideLoop`.
2. No match for a capture selector. spec.txt section 14 and 15 say the result is
   `f`. The interpreter emits the single character `f` into the output string.
3. Shift counts. spec.txt section 8 says the count must be 0 to 63 but describes
   no error. The interpreter raises `MathError: InvalidShiftCount` outside that
   range. `bit-shl 1 63` is a separate `IntegerOverflow`.
4. Reserved names. spec.txt section 2 says reserved literals cannot be binding
   names and separately that reserved special forms cannot either. The
   interpreter rejects both, but the reported error differs: literals give
   `TypeError: let name must be an identifier`, while form and built-in names
   give `TypeError: reserved name cannot be bound`.
5. `expect` and `and`, `or`, `not` were not specified at all. Sections 11.3 to
   11.6 here are new.
6. The immediate-call form of `fn` was not specified at all. Section 6.1 here is
   new.
7. The rule that access paths must be rooted at a variable was not specified.
   Section 3.1 here is new.
8. Specifier list. `%h` fixed widths exist, `%o` fixed widths do not, and `%f`
   has no width or precision. The doubled `FormatError:` prefix on some messages
   is documented in Section 15.11.

### Implementation behaviour worth knowing

- Shebang line offset. Error line numbers ignore the removed shebang line. The
  REPL's line numbers include it. spec.txt section 20 documents the REPL side
  only.
- `r+` and `w+` write position. A write goes to the underlying file position,
  which the buffered reader has already advanced past, so an interleaved write
  lands after the prefetched block. spec.txt section 13 says only "the current
  position".
- `io.close` on descriptor 1 or 2 succeeds and removes it, after which all writes
  to it fail.
- `pow 2 -1` reports `IntegerOverflow` rather than anything about exponents.
- `mod` with a float operand is a `TypeError`.
- `FormatError` messages on seven sites already begin with `FormatError: `, so
  the printed message repeats the prefix. Section 15.11 lists them.
- `%s` escapes a control character below 0x1F as JSON `\u00XX` when the string is
  nested, while the reader drops the backslash of an unknown escape, so such a
  string does not survive a `%s` and read-back round trip. `%q` and `%x` leave
  the character raw and do survive.
- The lexer silently drops the backslash of an unknown escape, so `"\q"` is the
  string `q`.
- `def:` in the REPL has a third message, `defined outside the current source`,
  which spec.txt section 20 does not list.
- A decoded JSON object keeps its keys in alphabetical order, not in document
  order.
- A slice or a multi-index selector must be the last step of a postfix chain.
  Section 1.3 here is new.
- `:i` scope words are `session`, `enclosing` and `outer`.
