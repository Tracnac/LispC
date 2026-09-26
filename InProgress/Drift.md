# Drift audit — `spec.txt` ↔ `src/` ↔ `README.md` ↔ `todo`

Audit of incoherence between the Small Lisp implementation, its French specification
(`spec.txt`), the English summary (`README.md`), and the engineering scratchpad (`todo`).
No source file was modified in the course of this audit.

## Baseline

- `cargo build --release` — up to date.
- `cargo test --release` — **477 passed, 0 failed**.
- Binary probed: `target/release/small-lisp`.
- Working tree at audit time: `2bf172a`, dirty only from local edits
  (` M examples/quick.lisp` content deleted, ` M todo` one line appended).

Test distribution (477 `#[test]` fns):

| file                        | tests | file                     | tests |
| --------------------------- | ----: | ------------------------ | ----: |
| `src/tests/comparisons.rs`  |   107 | `src/tests/modules.rs`   |    20 |
| `src/tests/strings.rs`      |    98 | `src/tests/repl.rs`      |    20 |
| `src/tests/core.rs`         |    62 | `src/tests/semantics.rs` |    13 |
| `src/tests/control_flow.rs` |    50 | `src/tests/io.rs`        |    12 |
| `src/tests/arithmetic.rs`   |    41 | `src/tests/errors.rs`    |     8 |
| `src/tests/collections.rs`  |    35 | `src/tests/http.rs`      |     6 |
|                             |       | `src/tests/types.rs`     |     5 |

## How findings were verified

Every claim marked **[probe]** was executed against the release binary. The language has no
`println` — output goes through the `io` module on descriptor 1 — so a prelude is used:

```sh
cat > /tmp/pre.lisp <<'EOF'
(use "io")
(let p (fn (x) (io.write 1 ($ "%v\n" x))))
EOF
cat /tmp/pre.lisp /tmp/probe.lisp | ./target/release/small-lisp
```

REPL claims require a controlling terminal and cannot be driven over stdin; they were verified
with a Python `pty` harness that makes the pty the child's controlling terminal
(`pty.openpty()` + `TIOCSCTTY` in a `setsid` child, program stdout redirected to a file so tty
traffic and real stdout can be separated).

Anything **not** probe-verified is labelled _code-reading-derived_.

---

## Summary

| class                                 | count | severity                                              |
| ------------------------------------- | ----: | ----------------------------------------------------- |
| A. spec.txt contradicts the code      |    17 | high — the spec is the normative document             |
| B. spec.txt is incoherent with itself |    10 | high — two mutually exclusive statements for one rule |
| C. README.md contradicts the code     |     7 | medium                                                |
| D. README.md ↔ spec.txt gaps          |     4 | medium — undocumented surface                         |
| E. `todo` staleness                   |     8 | low                                                   |
| F. Documented but untested            |    12 | medium — these hide A-class bugs                      |
| G. Code defects surfaced by the audit |     7 | high — real user-facing bugs                          |

The most serious items:

- **G2** — `%s` emits JSON escapes the Lisp reader cannot parse back; `(eval ($ "%s" [s]))`
  silently _expands_ strings containing control bytes. Data corruption.
- **G3** — a deferred Ctrl-C in the REPL **silently swallows the next line typed**.
- **G4** — `r+`/`w+` writes land at the reader's prefetch offset, not the documented
  "current stream position". Documented in two places; neither is true.
- **G1** — all 12 `$` error sites print a doubled `FormatError: FormatError:` prefix.
- **B1** — `spec.txt` states both "reserved names cannot be binding names" and "reserved names
  _can_ be binding names", three lines apart.
- **A15** — the §3 grammar cannot derive `(io.write 1 "x")`, `box[1]`, or `(add 1 2)[1]`.

---

## A. spec.txt contradicts the code

### A1 — the identifier regex is fiction

`spec.txt:26` — "Identifiants — Regex : `[A-Za-z0-9_-]+`, ASCII uniquement."

**False.** The lexer's symbol terminator set is `!"()[]{}:,.^"';#` (`src/main.rs:272`); it never
validates the symbol body. The only constraint on binding names is the reserved-name check at
`src/main.rs:1358`.

```lisp
(let * 1)      ; binds
(let a+b 1)    ; binds
(let < 1)      ; binds
```

The documented regex describes nothing the code does. Either the lexer must validate, or the
sentence must be replaced with the actual rule (any run of characters not in the terminator set).

### A2 — `-NaN` is not a reserved literal

`spec.txt:48` lists `-NaN` among the reserved literals that cannot be identifiers. There is no
`-NaN` literal in the parser; `spec.txt:27`'s list correctly omits it.

```lisp
(let -NaN 1)   ; binds
(p -NaN)       ; => Int(1)
```

The code matches line 27, not line 48. (See also B2 — the two lists disagree with each other.)

### A3 — module member validation is stricter than documented

`spec.txt:70` — "Tout **membre appelable** d'un module doit être un descripteur `{_: callable spec: ...}`."

**False, and the code is stricter than the text.** `validate_module` (`src/main.rs:2258`) rejects
_any_ non-descriptor member, callable or not. Pinned by `src/tests/modules.rs:472-475`.

```lisp
(let m {a: 1})
(use "m")     ; TypeError: module members must be callable descriptors
```

A module therefore can never carry a data field. That is a real (if defensible) restriction that
is stated nowhere.

### A4 — `r+`/`w+` write position

`spec.txt:280-281` — "Pour les modes mixtes (`r+`, `w+`, `a+`), lectures et écritures partagent le
même fichier et la même position de flux ; une écriture se fait à la position courante, sauf `a+`
qui écrit toujours à la fin du fichier." Mirrored by `README.md:186-188`.

**False, and it is a real data-placement bug, not just imprecise wording.**
`src/modules/io.rs:266` does `stream.reader.get_mut().write_all(...)` with no `seek`; the
`BufReader`'s prefetch has already advanced the OS cursor to EOF.

```sh
printf 'aaa\nbbb\nccc\n' > /tmp/rplus.txt      # 12 bytes
# open "file:/tmp/rplus.txt?mode=r+"; io.read  => "aaa"  (consumes 4 bytes)
# io.write "ZZZ"
```

Result: `aaa\nbbb\nccc\nZZZ` (15 bytes). The write landed at offset 12, not 4.

`src/tests/io.rs:179-181` carries the comment "The write lands at the read position (EOF)" — true
only by accident, because its fixture is a single 7-byte line with no prefetch to speak of.

### A5 — `ne` with zero or one argument

`spec.txt:143` — "avec zéro ou un argument, le résultat est vrai."

**False for `ne`.**

```lisp
(p (ne))      ; => Bool(false)
(p (ne 1))    ; => Bool(false)
```

`spec.txt:249` states the opposite (`(ne)` et `(ne x)` → `f`) and `src/tests/core.rs:111` pins it.
Line 143 is a copy-paste of the `eq` / order-comparison sentence.

### A6 — `eq` and `ne` are neither chained nor all-distinct

`spec.txt:142-143` — "`eq` et les comparaisons d'ordre sont **chaînées** ; `ne` vérifie que **toutes
les valeurs sont distinctes**."

**Both clauses are wrong.** The code computes `eq` = all-pairs-equal (a conjunction over pairs, not
a left-to-right chain) and `ne` = the exact negation of `eq`.

| expression   | result                                                       |
| ------------ | ------------------------------------------------------------ |
| `(eq 1 1 1)` | `true`                                                       |
| `(ne 1 1 1)` | `false` ← "all distinct" would be `false` too, but see below |
| `(eq 1 2 1)` | `false`                                                      |
| `(ne 1 2 1)` | `true` ← **"all distinct" would be `false`**                 |
| `(eq 1 2 3)` | `false`                                                      |
| `(ne 1 2 3)` | `true`                                                       |

Pinned by `src/tests/comparisons.rs:425-443` (`eq_is_pairwise_not_just_adjacent`,
`eq_and_ne_are_exact_complements`). `spec.txt:248-250` is the correct statement.

### A7 — `break` inside a `match` inside a function called from a loop

`spec.txt:265-266` — "Un `break` dans un `match` situé dans une **fonction appelée depuis une loop**
ne sort que de la fonction (les boundaries de scopes de `fn` s'appliquent comme pour la profondeur
de loop)."

**False — it is a hard error, not a scoped exit.**

```lisp
(let g (fn (x) (match t (break))))
(loop (p (g 5)) (break))
```

```
<stdin>:3:25: BreakOutsideLoop
(let g (fn (x) (match t (break))))
                        ^
call trace:
  g at <stdin>:4:10
```

`Flow::Break` (`src/main.rs:2063`) is not caught by `invoke` (`src/main.rs:2422`); only the literal
`loop` form catches it. The whole "fn scopes apply like loop depth" clause describes behavior that
does not exist.

### A8 — the field-access desugaring is not a program

`spec.txt:101` — `field-access := identifier ("." identifier)*  ; sucre pour (. (. a b) c)`

**The desugaring does not parse.**

```lisp
(. box 1)   ; ParseError: unexpected token Dot
```

`.` is only ever a binary AST node (`ExprKind::Field`); there is no `call` form for it. The comment
should be dropped, not repaired.

### A9 — `^array` describes a pre-Model-P language

`spec.txt:171` — "`^` sur un tableau entier est autorisé et permet la mutation en place dans une
fonction, l'array passé par référence pointe vers la même structure sous-jacente que celle de
l'appelant, toute mutation via des opérations sur ce paramètre (**ajout**, modification d'un
élément, etc.) est visible côté appelant."

**Stale.** The language has **no append operation and no length mutation of any kind**. `set`
replaces an element; nothing grows a composite. The word "ajout" describes an operation that does
not exist. `README.md:170-177` documents the actual model correctly ("each write rebuilds the
snapshot chain along the path"); the spec is the one that is behind.

### A10 — REPL echo: wrong stream, wrong format

`spec.txt:443` — "le résultat de la dernière forme (s'il n'est pas `_`) est affiché **sur stdout**
au format lisp."

**Wrong on both counts.**

_Stream_ — with program fd 1 redirected to a file, that file received only the program's own
`io.write` output; the echoed result went to the tty.

_Format_ — `repl_echo` (`src/main.rs:1466-1472`) prints `<fn>` / `<native fn>`, not the lisp form:

```lisp
(let h (fn (x) x))
;; type `h` at the prompt
;; echoes:  <fn>          "au format lisp" would be  (fn h (x))
```

### A11 — Ctrl-C cancels the _wrong_ line

`spec.txt:473-476` and `README.md:129-131` — "Ctrl-C pendant une session : la ligne en cours est
annulée, un message « repl: interrupted (:c continues, :q quits) » est affiché et la session
continue."

**The interrupt is deferred and eats the _next_ line instead.** The check sits at the top of the
session loop (`src/main.rs:1936`), _after_ `console.read_line` has already returned.

Observed over a pty: SIGINT at an idle prompt prints **nothing**. Then typing `(add 1 2)`:

```
/tmp/pty_prog.lisp:4> (add 1 2)
repl: interrupted (:c continues, :q quits)
/tmp/pty_prog.lisp:4>
```

The line's result (`3`) is never echoed. The user's input is silently discarded.

`src/tests/repl.rs:240-255` cannot catch this — it drives the `REPL_INTERRUPT` hook, which feeds
the _same_ top-of-loop branch. See F2.

### A12 — the §17 error list is missing three categories

`spec.txt:412-414` lists 16 names. The `Error` enum (`src/main.rs:117-135`) has 19 variants.

| missing            | documented at                                   |
| ------------------ | ----------------------------------------------- |
| `ModuleError`      | `spec.txt:66`, `README.md:85` — but not in §17  |
| `RecursionError`   | `spec.txt:154`, `README.md:34` — but not in §17 |
| `ExpectationError` | **nowhere at all**                              |

`ExpectationError` is the worst of the three: `spec.txt:33` lists `expect` as a special form and
stops there; `README.md:213` describes its behaviour without ever naming the error it raises.

### A13 — commas are whitespace everywhere, not only in `[]`/`{}`

`spec.txt:22` — "Virgules optionnelles dans `[]` et `{}` : traitées comme du whitespace si présentes."

The lexer treats `,` as a plain word separator (`src/main.rs:272` terminator set), everywhere.

```lisp
(p (add 1 2 , 3))   ; => Int(6)
(p (sub 5 , 1))     ; => Int(4)
(p (,))             ; => Null
```

Not a bug — an under-specified and needlessly restrictive doc.

### A14 — `match_depth` is threaded but never read

`spec.txt:404-405` — "Le code évalué hérite de la profondeur de boucle/**match** de l'appelant."

`match_depth` is passed through `eval` / `call` / `apply_index` / `invoke`
(`src/main.rs:1112`, `1144-1314`) and incremented at `src/main.rs:2085` (`m + 1`), but **no
expression anywhere reads it**. It is a dead parameter. The `loop_depth` half of the sentence is
real; the `match_depth` half is vestigial. (Code-reading-derived: provable by inspection, since
the value is never compared, printed, or branched on.)

### A15 — the grammar cannot derive real programs

`spec.txt:96-102`:

```
call         := "(" symbol form* ")"
field-access := identifier ("." identifier)*
ref          := "^" postfix-access
postfix-access := field-access ("[" selector "]"*
```

Four gaps:

1. `call` requires a `symbol` head, so `(io.write 1 "x")` — a `Field` head — is underivable.
   Descriptor calls are used throughout the spec and the examples.
2. `postfix-access` is reachable **only** from `ref`, so plain `box[1]`, `box[1].c`, `a.b[1].c`
   and `(add 1 2)[1]` are all underivable, even though every one is legal.
3. `selector` is used twice and **never defined**.
4. `field-access` is a non-terminal but no production references it outside `postfix-access`.

### A16 — bit-shift error surface is undocumented and incomplete

`spec.txt:216` — "les décalages exigent un compte entre 0 et 63."

```lisp
(p (bit-shl 1 64))   ; MathError: InvalidShiftCount     <- name documented nowhere
(p (bit-shl 1 63))   ; MathError: IntegerOverflow       <- bit-shl is NOT in spec.txt:212's overflow list
```

A `bit-shl` overflow is a distinct failure mode from `add`/`sub`/`mul`/`pow`, and `InvalidShiftCount`
appears in no document.

### A17 — `break` and `continue` check arity in different orders

```lisp
(p (break 1 2))            ; BreakOutsideLoop
(loop (if t (break 1 2) 0)) ; ArityError: break expects zero or one argument
(continue 1 2)             ; ArityError: continue expects 0 arguments, got 2
```

`break` tests `l == 0` first (`src/main.rs:2057-2061`); `continue` tests arity first
(`src/main.rs:2069`). So `break`'s arity error is unreachable outside a loop, while `continue`'s is
always reachable. Neither ordering is documented.

---

## B. spec.txt is incoherent with itself

### B1 — reserved names: two mutually exclusive rules, three lines apart

- `spec.txt:45-47` — "Les noms de formes spéciales, de builtins stricts et d'interaction ci-dessus
  sont réservés : **ni un `let` ni un paramètre de fonction ne peut les utiliser comme identifiant
  (erreur)**."
- `spec.txt:51-53` — "**Les noms peuvent toutefois être utilisés comme noms de liaison dans les
  autres contextes**, sous réserve des littéraux réservés ci-dessus."

Both cannot hold. The code follows 45-47:

```lisp
(let add 1)          ; TypeError: reserved name cannot be bound: `add`
(let g (fn (eq) 1))  ; TypeError: reserved name cannot be used as a parameter: `eq`
```

All 34 names in `RESERVED_NAMES` (`src/main.rs:1334`) are rejected, and they agree exactly with
`spec.txt:32-38`. **Line 51-53 must be deleted.**

### B2 — reserved literals: two different lists

- `spec.txt:27` — `t`, `f`, `_`, `NaN`, `+NaN`, `Inf`, `+Inf`, `-Inf` (8 names)
- `spec.txt:48` — `t`, `f`, `_`, `NaN`, `Inf`, `+Inf`, `-Inf`, `+NaN`, `-NaN` (9 names)

The code matches line 27. See A2.

### B3 — `eq`/`ne`: two different definitions

`spec.txt:142-143` ("chaînées" / "toutes distinctes") vs `spec.txt:248-250` ("égaux deux à deux" /
"négation exacte de `eq`"). See A6.

### B4 — `(ne)` / `(ne x)`: two different results

`spec.txt:143` ("vrai") vs `spec.txt:249` ("`f`"). See A5.

### B5 — the width-error taxonomy contradicts the type-error taxonomy

- `spec.txt:373-374` puts "une largeur binaire autre que 8, 16, 32 ou 64" in the generic
  `FormatError` bucket.
- `spec.txt:377-378` reserves the `FormatTypeError` sub-message for **type** incompatibility with
  `%d`, `%b`, `%h`, `%o`, `%f`, `%j`.

The code uses `FormatTypeError` for widths and a bare message for overflow:

```lisp
(p ($ "%7b" 5))                  ; FormatError: FormatTypeError: %7b supports widths 8, 16, 32, or 64
(p ($ "%99999999999999999999b" 5)) ; FormatError: invalid binary width
```

Two different taxonomies for one error class. (`src/main.rs:2752`, `2768` vs the overflow path.)

### B6 — "callable member" vs "syntax validation"

`spec.txt:70` constrains "membre **appelable**"; `spec.txt:84-86` says `use` "valide la **syntaxe**
du module" and that `use` is "la porte de validation syntaxique". "Syntax" implies shape, which
would permit data members. The code permits only descriptors. See A3.

### B7 — §17 contradicts §2.1 and §6

`spec.txt:412-414` omits `ModuleError` and `RecursionError`, both defined elsewhere in the same
document (`spec.txt:66`, `spec.txt:154`). See A12.

### B8 — the §9 coercion table is incomplete

`spec.txt:238`'s `$ (%b/%h/%o)` row lists only the **binary** fixed widths (`%8b %16b %32b %64b`).
The hexadecimal fixed widths (`%8h %16h %32h %64h`) are documented at `spec.txt:332-334` and
`README.md:211` but are missing from the table.

### B9 — the documented prompt is unreachable; the reachable one is undocumented

`spec.txt:438-439` documents `repl> ` for "la racine sans contexte". That branch
(`src/main.rs:1566-1567`) requires `source: None` or an execution span outside the registered source.
`interpreter_main` (`src/main.rs:3328`) **always** pushes a source, so `repl> ` is unreachable from
the CLI. A real prompt is `file:line> ` or `label@file:line> `.

Meanwhile the fourth form, `label> ` (`src/main.rs:1570`), _is_ reachable and appears in no
document.

### B10 — §20 promises a caret column that lexer errors do not get

`spec.txt:464-465` — "Les erreurs d'une ligne sont affichées (fichier `<repl>`, ligne 1, **colonne
pointée**)."

Only the _parser_ sets `PARSE_ERROR_SPAN` (`src/main.rs:495`); the lexer never does. So for a
lexer error, `repl_diagnostic` (`src/main.rs:1944`) reads whatever stale span the program's own
parse left behind, then clamps it with `.min(line.len())`. Typing `"abc`:

```
<repl>:1:5: ParseError: unterminated string
"abc
    ^
```

Column 5 on a 4-character line, caret one past the end.

---

## C. README.md contradicts the code

### C1 — the `r+`/`w+` write-position claim

`README.md:186-188`. Same defect as A4.

### C2 — `:i name` overstates what it shows

`README.md:127-128` — "`:i name` shows one binding's value, type, binding scope and **definition
site**, with the shadow chain when one exists".

The definition site exists **only for functions**:

```
:stty
filefn = (fn filefn (x))   function   scope 1 (enclosing)
  def: /tmp/pty_prog.lisp:1
```

`:i` on the `io` module struct shows no `def:` line. `spec.txt:455-458` states this correctly.

### C3 — REPL echo is silent about both stream and format

`README.md:123-124` — "the last non-null result is echoed". Silent on the `<fn>` / `<native fn>`
substitution and on the fact that it goes to the tty, not stdout. See A10.

### C4 — the Ctrl-C claim

`README.md:129-131`. Same defect as A11.

### C5 — `expect`'s error text is undocumented and uses the debug renderer

`README.md:213` — "checks that its first expression equals its second expression and returns `t`.
An optional final string describes the assertion when it fails."

```lisp
(p (expect 1 2))        ; ExpectationError: expectation failed: expected Int(2), got Int(1)
(p (expect 1 2 "boom")) ; ExpectationError: boom: expected Int(2), got Int(1)
```

Neither the debug-style value form (`Int(2)`) nor the `comment: expected…, got…` shape is
documented anywhere. See G6.

### C6 — two's complement is documented for `%b` but silently true for `%o` and `%h`

`README.md:211` — "Fixed-width binary forms render the low bits of the integer, including two's
complement representations for negative integers."

The **unfixed** `%o` and `%h` also emit two's complement, unmentioned:

```lisp
(p ($ "%o" -1))   ; Str("1777777777777777777777")
(p ($ "%h" -1))   ; Str("ffffffffffffffff")
```

(`spec.txt:329-330`, `333-334` document two's complement for the fixed-width **binary** and
**hex** forms only.)

### C7 — a backward cross-reference

`README.md:96` says "see Formatting **above**". `## Formatting and assertions` is at line **209**,
i.e. below.

---

## D. README.md ↔ spec.txt gaps

### D1 — the `str` module has no behavioral documentation anywhere

`spec.txt:58` names `str.upper` / `str.lower` in passing, inside a sentence about `use`.
`README.md:82` lists `str` among the natives. Neither describes behaviour, arity, or errors, and
**neither document has a section for it** — unlike `io` and `http`.

The implementation uses Rust's full Unicode mapping, which changes string length:

```lisp
(p (str.upper "straße éà"))   ; Str("STRASSE ÉÀ")     <- ß → SS, 7 graphemes → 8
(p (str.lower "ÉÀÜ"))          ; Str("éàü")
(p (str.upper 5))              ; TypeError: module function argument 1 expects string, got int
```

A surprising edge with zero documentation and zero coverage.

### D2 — `expect` has one sentence in the entire spec

`spec.txt:33` lists it as a special form and stops. `README.md:213` is the only behavioural
statement, and neither names `ExpectationError` (A12). Of the 15 special forms, this is the
largest documentation hole.

### D3 — the `%s` escape set is unspecified

`spec.txt:209` groups `%s` with `%q` / `%x` / `%j` as "sérialisation et comparaison", and
`spec.txt:320` says `%x` reuses `%q`'s escape rules — but nothing states which escapes `%s` uses
for the nested strings it quotes. It is **not** `%q`'s set, and the difference is a live bug (G2).

### D4 — the recursion bound is asymmetric

`README.md:33-35` mentions `RecursionError` without the numeric bound; `spec.txt:154` gives
`MAX_CALL_DEPTH = 2048`. Minor.

---

## E. `todo` staleness

| #      | line                             | claim                                                                           | reality                                                                                                                                                                                                                                |
| ------ | -------------------------------- | ------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **E1** | `todo:49`, `todo:86`, `todo:123` | "463 tests", "473 tests green", "470 tests green"                               | Three mutually inconsistent numbers in one file, all wrong. Actual: **477**.                                                                                                                                                           |
| **E2** | `todo:74`                        | "the per-node `LAST_ERROR_SPAN` write (eval entry, **main.rs:1110**)"           | `src/main.rs:1117`                                                                                                                                                                                                                     |
| **E3** | `todo:84`                        | "Descriptor (**2226**)"                                                         | `src/main.rs:2231` (in `call`), `src/main.rs:2245` (in `invoke_operator`)                                                                                                                                                              |
| **E4** | `todo:84`                        | "recursion-guard (**2442**)"                                                    | `src/main.rs:2458` (inside `invoke`, `src/main.rs:2422`)                                                                                                                                                                               |
| **E5** | `todo:85`                        | "(eval) **2169-2182**"                                                          | `src/main.rs:2175-2206`                                                                                                                                                                                                                |
| **E6** | `todo:110`                       | "`examples/quick.lisp` still uses the old one-step shape (user WIP, untouched)" | **Wrong twice.** The file is now 0 bytes. And at `HEAD` it was `(use "io")(io.write 1 ($ "Hello %s %~%1.1" "Yvan" "(a)(b)" "xxabyyabzz"))` — the _native two-step_ form, never the removed `(use (let …))` shape this entry describes. |
| **E7** | `todo:41`                        | "13 tests in src/tests/semantics.rs"                                            | ✅ correct                                                                                                                                                                                                                             |
| **E8** | `todo:59`                        | "innermost 40 frames"                                                           | ✅ correct (`src/main.rs:3249`)                                                                                                                                                                                                        |

---

## F. Documented but untested — the coverage illusions that hide the A-class bugs

### F1 — `src/tests/repl.rs:81-88` asserts neither half of its own name

`repl_accepts_an_optional_label_and_eof_returns_null` passes the label `"breakpoint"` and never
inspects the prompt; it asserts the program's value is `2`, not `Null`. The prompt logic
(`src/main.rs:1560-1577`) is entirely unasserted — which is why B9 went unnoticed.

### F2 — `src/tests/repl.rs:240-255` exercises the wrong branch

`repl_interrupt_is_contained_to_the_session_and_restored_on_exit` sets `REPL_INTERRUPT`, which
`src/main.rs:1936` reads at the **top** of the loop. The real Ctrl-C path — signal →
`INTERRUPTED` atomic, checked only _after_ `read_line` returns — is never driven. This is exactly
A11.

### F3 — zero `/dev/tty` coverage

`src/tests/repl.rs:91-99` asserts the **negative**
(`panic!("expected the queued test console, not /dev/tty")`). The entire `ReplConsole::Tty` arm,
including the stdout-vs-tty routing that A10 and C3 contradict, is untested.

### F4 — the doubled `FormatError:` prefix is structurally invisible

`src/tests/strings.rs:270, 274, 864, 868` assert `message == "FormatArityError"`; `:757, 836`
assert `message.contains("match index 2 out of range")`. Both assert the **inner** message, never
the rendered one — so the `Display`-level doubling in G1 cannot fail a test.

### F5 — `FormatTypeError` is never asserted by name

`grep` over `src/tests/` → 0 hits. The `%7b` / `%h` width path (`src/main.rs:2752`, `2768`) and the
`"invalid binary width"` path have **no test at all**.

### F6 — `expect`'s arity errors are untested

`expect expects 2 or 3 arguments, got N` (`src/main.rs:2118`) appears in no test. `expect` only
ever appears on the success path.

### F7 — `%32b` / `%64b` with a negative value are untested

`src/tests/strings.rs:48` covers `%8h %16h %32h %64h` negative; `:433` and `:439` cover `%32b` /
`%64b` with `5` only.

```lisp
(p ($ "%32b" -1))   ; Str("11111111111111111111111111111111")   <- works, untested
(p ($ "%64b" -1))   ; Str("1111111111111111111111111111111111111111111111111111111111111111")
```

### F8 — `eq` function identity is untested

`spec.txt:243` — "Fonctions : égalité par identité (même closure physique), deux fn au code
identique mais définis séparément → `f`."

```lisp
(let a1 (fn (x) x))  (let a2 (fn (x) x))
(p (eq a1 a2))   ; Bool(false)
(p (eq a1 a1))   ; Bool(true)
```

No test.

### F9 — `eq`/`ne` on mixed non-numeric types are untested

`spec.txt:229` — "Types non numériques différents → f (**jamais d'erreur**)."

```lisp
(p (eq "a" 1))   ; Bool(false)     no TypeError
(p (ne "a" 1))   ; Bool(true)
```

No test.

### F10 — `lt`/`gt`/`le`/`ge` `TypeError` is untested

`spec.txt:230` — "Numérique uniquement … Types non numériques → TypeError."

```lisp
(p (lt 1 "a"))   ; TypeError: expected number
```

No test.

### F11 — `bit-shl` overflow is untested

`(bit-shl 1 63)` → `MathError: IntegerOverflow`. Only `InvalidShiftCount` is covered
(`src/tests/arithmetic.rs:58`).

### F12 — the §14 error-reporting half is nearly untested

No test asserts _where_ a `$` error points. Only the three `LAST_ERROR_SPAN` regressions in
`src/tests/errors.rs` touch attribution at all.

---

## G. Code defects surfaced by the audit

These are implementation bugs, not documentation drift.

### G1 — every `$` error message prints a doubled `FormatError:` prefix

`Error::Format`'s `Display` arm adds `FormatError: ` (`src/main.rs:150`), but **12** raise sites
bake the same prefix into the message itself.

| site                                                                       | kind               |
| -------------------------------------------------------------------------- | ------------------ |
| `src/main.rs:2588`, `2591`, `2690`, `2736`                                 | `FormatArityError` |
| `src/main.rs:2596`, `2602`, `2700`, `2707`, `2714`, `2721`, `2727`, `2901` | `FormatTypeError`  |

```
$ ($ "%7b" 5)     → FormatError: FormatTypeError: %7b supports widths 8, 16, 32, or 64
$ ($ "%d" "x")    → FormatError: FormatTypeError: %d expects integer
$ ($ "%q" 5)      → FormatError: FormatTypeError: %q expects string
$ ($ "%s" 5)      → FormatError: FormatArityError
$ ($ "%~%2.1" "U~(a)|(b)" "ab")
                  → FormatError: FormatError: match index 2 out of range
```

**Fix:** strip the literal prefix at all 12 sites. Invisible to the suite because F4 asserts the
inner string.

### G2 — `%s` emits JSON escapes the Lisp reader cannot parse back → data corruption

`render_nested` (`src/main.rs:1057-1060`) routes nested strings through **`json_string`**
(`src/main.rs:2866-2880`), which emits `\u00XX` for any char ≤ `0x1F`. The reader's escape arm is
`other => other` (`src/main.rs:353`) — it **drops the backslash**. So `\u0001` reads back as the
five literal characters `u0001`.

```lisp
(let s (str.lower 'A<BEL>B<DEL>C'))   ; raw string, 4 bytes: 61 01 62 7f 63
($ "%s" [s])           ; => ["a<BEL>b<DEL>c"]     <-- raw bytes inside the quotes
(eval ($ "%s" [s]))    ; => ["au0001b<DEL>c"]     <-- 4 bytes became 5
```

`%q` and `%x` are **correct** — they use `lisp_string` (`src/main.rs:2882-2895`), which matches
`spec.txt:316-317` exactly (`"`→`\"`, `\`→`\\`, NL→`\n`, CR→`\r`, TAB→`\t`, everything else
verbatim) and re-parses identically. Verified round-trip for the 5-escape set:

```lisp
(let s "A\tB\nC\\D\"E")
($ "%s" [s])        ; => ["A\tB\nC\\D\"E"]
(eval ($ "%s" [s])) ; => ["A\tB\nC\\D\"E"]        <- correct
($ "%q" s)          ; => "A\tB\nC\\D\"E"          <- correct
```

**Fix:** `render_nested` should call `lisp_string`, not `json_string`. This also closes the
`%s`-vs-`%q` escape-family gap in D3.

### G3 — a deferred Ctrl-C silently swallows the next REPL line

`src/main.rs:1936` checks `INTERRUPTED` **after** `console.read_line` has already returned a line,
then `continue`s — discarding it. A SIGINT at an idle prompt produces no message; the message
appears only after the user has typed their next line, which then vanishes. See A11 for the
transcript.

**Fix:** check `INTERRUPTED` immediately after the blocking read returns, before evaluating or
dispatching; or make the read itself interruptible so the cancel applies to the line being typed.
Add a test that drives the `INTERRUPTED` atomic rather than the `REPL_INTERRUPT` hook.

### G4 — `r+`/`w+` writes land at the reader's prefetch offset

`src/modules/io.rs:266`. Documented as "at the current stream position" by both `spec.txt:280-281`
and `README.md:186-188`; the code does neither that nor anything else well-defined. See A4.

**Fix options:** (a) drop the `BufReader` for writable handles and use a single `File` with an
explicit `seek` per operation; (b) track a logical position alongside the buffer and `seek` before
each write. (a) is simpler; the `ReadableStream` comment at `src/modules/io.rs:16-20` justifies the
buffer for read-only buffer reuse, which is unaffected.

### G5 — the lexer silently deletes the backslash of unknown escapes

`src/main.rs:353` — `other => other`.

```lisp
(p "a\qb")            ; Str("aqb")
(p "a\0b")            ; Str("a0b")
(p "a\eb")            ; Str("aeb")
(p (str.lower "\q"))  ; Str("q")
```

No error, no warning, no round-trip. `spec.txt:6` says only "échappement `\n`, `\t`, etc." — the
accepted set is never enumerated and the silent-drop behaviour is nowhere documented.

**Fix:** either enumerate the six accepted escapes at `spec.txt:6` and keep the permissive
fallthrough, or raise a `ParseError` on an unknown escape.

> This is what made an earlier `%s` round-trip probe of this audit _look_ like a formatting bug.
> It was this defect, not G2. G2 is real but narrower — control characters outside the documented
> 5-escape set.

### G6 — `expect`'s message uses the debug renderer

`src/main.rs:2131-2135` uses `debug_render`, so failures read `expected Int(2), got Int(1)` —
inconsistent with `$`'s `%s` and with every other user-facing value rendering in the language. The
semantics are right; only the rendering is off. See C5.

### G7 — smaller undocumented failure modes

| expression                        | result                                  | note                                                                                                                                                                                                 |
| --------------------------------- | --------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `(mod 7.0 2)`                     | `TypeError: mod requires integers`      | `spec.txt:223` says an int/float mix promotes to float; `spec.txt:213-214` describes `mod` as C-like with no integer restriction. Add the rule or relax the code.                                    |
| `(pow 2 -1)`                      | `MathError: IntegerOverflow`            | **Misnamed** — nothing overflowed; the result is `0.5`. Needs a `NegativeExponent` (or plain `Math`) case.                                                                                           |
| `(bit-shl 1 64)`                  | `MathError: InvalidShiftCount`          | the name appears in no document.                                                                                                                                                                     |
| `(bit-shl 1 63)`                  | `MathError: IntegerOverflow`            | `bit-shl` is missing from `spec.txt:212`'s overflow list.                                                                                                                                            |
| `box [1]` (whitespace before `[`) | `ArityError: function expects 1, got 2` | `postfix-access` requires **strict adjacency**; a whitespace slip silently becomes a function call instead of a parse error. Worth a diagnostic, or at least an adjacency note in the grammar (A15). |

---

## Verified correct — no action needed

Confirmed accurate and mutually consistent across code, spec, and README:

- **All `use` two-step semantics** — `ModuleError` per scope, child-scope re-registration,
  `DuplicateBindingError` for natives, `NameError "module \`X\` is not defined"`, `TypeError
  "expected string"`for`(use (let m {…}))`. Matches `spec.txt:56-69`, `README.md:82-85`.
- **All Model-P reference rules** — `box[2][1]` write-through mutating `b`; `set box[2] 7` replacing
  the alias; root rebind re-targeting; incompatible rebind → `indexing requires an array`; alias
  revival; `(let q p)` isolation vs `(let q (^ p))` two-hop chains; all eight `cyclic reference`
  cases. `spec.txt:184-213` and `README.md:139-168` agree.
- `spec.txt:162-165` (the `incr` example) and `spec.txt:172-182` (the `user` example).
- `spec.txt:324-338`: `%d`, `%b`, `%h`, `%o`, `%f`, `%j`, `%t`, `%v`, `%%`, `%M.C`, `%N`, and the
  `%~` option letters and defaults.
- `MAX_CALL_DEPTH` allows exactly 2048 nested calls before `RecursionError`; the trace cap is 40
  frames (`src/main.rs:3249`).
- `RESERVED_NAMES` ↔ `call` dispatch ↔ `spec.txt:32-38` all agree on the same 34 names.
- `<invalid reference>` at the root vs `null` in a nested `%s` position.
- `README.md:73-80`'s module example; the `"any"` wildcard rules; `$` arity and type errors.
- `README.md:139-177`'s performance note matches the implemented Model P.

---

## Suggested order of work

1. **G1, G2, G5** — small, localized, no documentation consequences. Start here.
2. **G3, G4** — real user-facing bugs that change _documented_ behaviour; pair each with its doc fix.
3. **B1, B2, B3, B4, B5** — delete the wrong sentences. The spec currently holds two mutually
   exclusive statements for reserved names, reserved literals, `eq`/`ne`, and `(ne x)`. Cheapest
   wins, most damaging to a reader.
4. **A15, A8** — rewrite the §3 grammar block so it can derive a real program.
5. **A3, A7, A9, A13, A14, A16, A17, A1, A2, A5, A6, A10, A12** — align the prose with the code,
   one or two lines each.
6. **F1, F2, F3** — the REPL suite's three blind spots. Fixing F1 and F2 first would have caught
   B9 and A11 immediately.
7. **E1-E6** — refresh `todo`'s test counts and line numbers; delete the `examples/quick.lisp`
   sentence.
8. **D1, D2** — write the missing `str` module section and the missing `expect` /
   `ExpectationError` section.
9. **F4-F12** — close the remaining coverage gaps, prioritised by whether they guard a documented
   rule (F8, F9, F10) or only a diagnostic (F4, F5, F12).
