;Check type and syntax
; Defined
(expect ($ "%t:%s" _ _) "null:")
(expect ($ "%t:%s" t t) "bool:true")
(expect ($ "%t:%s" f f) "bool:false")
; Integer
(expect ($ "%t:%s" 0 0) "int:0")
(expect ($ "%t:%s" 127 127) "int:127")
(expect ($ "%t:%s" -127 -127) "int:-127")
(expect ($ "%t:%s" 0x7F 127) "int:127")
(expect ($ "%t:%s" 0b01111111 127) "int:127")
(expect ($ "%t:%s" 0o177 127) "int:127")
; Float
(expect ($ "%t:%s" 0.0 0.0) "float:0")
(expect ($ "%t:%s" 1.5 1.5) "float:1.5")
(expect ($ "%t:%s" -1.5 -1.5) "float:-1.5")
; String
(expect ($ "%t:%s" "text" "text") "string:text")

; ============================================================
; Array
; ============================================================

; Basic array equality
(expect ($ "%t:%s" [1 2 3] [1 2 3]) "array:[1 2 3]")
(expect [1 2 3] [1 2 3] "Array equality")
(expect (eq [1 2 3] [1 2 3]) t "Array equality")
(expect (eq [1 2 3] [3 2 1]) f "Array order matters")
(expect (eq [1 [2 3]] [1 [2 3]]) t "Nested array equality")
(expect (eq [1 [2 3]] [1 [3 2]]) f "Nested array inequality")


; ============================================================
; Nested arrays / struct
; ============================================================

(let complex-struct
  {"name":"Yvan"
   "age":56
   "address":"1 rue de paris"
   "city":"Paris"
   "contact":{
     "gsm":"0102030405"
     "fax":"0102030406"
   }
   "score":[
     1
     2
     [21 [211 212] 22 23]
     3
   ]})

(expect
  complex-struct
  {"name":"Yvan"
   "age":56
   "address":"1 rue de paris"
   "city":"Paris"
   "contact":{
     "gsm":"0102030405"
     "fax":"0102030406"
   }
   "score":[1 2 [21 [211 212] 22 23] 3]}
  "Struct equality")

; ------------------------------------------------------------
; Struct fields
; ------------------------------------------------------------

(expect complex-struct.name
        "Yvan"
        "Get struct string field")

(expect complex-struct.age
        56
        "Get struct integer field")

(expect complex-struct.city
        "Paris"
        "Get struct field")

(expect complex-struct.contact.gsm
        "0102030405"
        "Get nested struct field")

(expect complex-struct.contact.fax
        "0102030406"
        "Get nested struct field")

; ------------------------------------------------------------
; Array indexing
; ------------------------------------------------------------

(expect complex-struct.score[1]
        1
        "Get 1st element of score")

(expect complex-struct.score[2]
        2
        "Get 2nd element of score")

(expect complex-struct.score[3]
        [21 [211 212] 22 23]
        "Get 3rd element of score")

(expect complex-struct.score[4]
        3
        "Get 4th element of score")

; ------------------------------------------------------------
; Nested array indexing
; ------------------------------------------------------------

(expect complex-struct.score[3][1]
        21
        "Get 1st element of nested score")

(expect complex-struct.score[3][2]
        [211 212]
        "Get 2nd element of nested score")

(expect complex-struct.score[3][3]
        22
        "Get 3rd element of nested score")

(expect complex-struct.score[3][4]
        23
        "Get 4th element of nested score")

(expect complex-struct.score[3][2][1]
        211
        "Get nested array element")

(expect complex-struct.score[3][2][2]
        212
        "Get nested array element")

; ------------------------------------------------------------
; Multiple-index selector
; ------------------------------------------------------------

(expect complex-struct.score[3][[1 3]]
        [21 22]
        "Select multiple elements")

(expect complex-struct.score[3][[1 4]]
        [21 23]
        "Select first and last elements")

(expect complex-struct.score[3][[2 3]]
        [[211 212] 22]
        "Select multiple nested elements")

(expect complex-struct.score[3][2][[1 2]]
        [211 212]
        "Select elements from nested array")

; ============================================================
; Negative indexes
; ============================================================

(expect complex-struct.score[-1]
        3
        "Last element")

(expect complex-struct.score[-2]
        [21 [211 212] 22 23]
        "Second-to-last element")

(expect complex-struct.score[3][-1]
        23
        "Last nested element")

(expect complex-struct.score[3][2][-1]
        212
        "Last deeply nested element")

; Negative ranges
(expect complex-struct.score[-2..-1]
        [[21 [211 212] 22 23] 3]
        "Last two elements")

(expect complex-struct.score[3][-3..-1]
        [[211 212] 22 23]
        "Last three nested elements")

; ============================================================
; Struct equality
; ============================================================

(expect
  (eq {"a":1 "b":2} {"a":1 "b":2})
  t
  "Struct equality")

(expect
  (eq {"a":1 "b":2} {"b":2 "a":1})
  t
  "Struct key order is irrelevant")

(expect
  (eq {"a":1 "b":2} {"a":1 "b":3})
  f
  "Struct values differ")

(expect
  (eq {"a":[1 2]} {"a":[1 2]})
  t
  "Deep struct equality")

(expect
  (eq {"a":[1 2]} {"a":[2 1]})
  f
  "Deep array order matters")

; Function
(expect ($ "%t:%s" (fn (x) x) (fn (x) x)) "function:<fn>")
; Reference
(expect ((let value 42)($ "%t:%s" (^ value) (^ value))) "ref:42")

;Formating
; Binary
(expect ($ "%b" 5) "101")
(expect ($ "%8b" 5) "00000101")
(expect ($ "%8b" -5) "11111011")
(expect ($ "%16b" 5) "0000000000000101")
(expect ($ "%16b" -5) "1111111111111011")
(expect ($ "%32b" 5) "00000000000000000000000000000101")
(expect ($ "%64b" 5) "0000000000000000000000000000000000000000000000000000000000000101")
(expect ($ "%8b" -1) "11111111")
(expect ($ "%16b" -1) "1111111111111111")
; Integer bases
(expect ($ "%d %h %o" 127 127 127) "127 7f 177")
; General values
(expect ($ "%s %f %t" "text" 1.5 1.5) "text 1.5 float")
(expect ($ "%j" {"name":"Ada" "values":[1 t _]}) "{\"name\":\"Ada\",\"values\":[1,true,null]}")
(expect ($ "%v" [1 "text"]) "Array([Int(1), Str(\"text\")])")
(expect ($ "100%%") "100%")

; Logical
; and
(expect (and t t) t)
(expect (and t f) f)
(expect (and f t) f)
(expect (and f f) f)

(expect (and 1 2) 2)
(expect (and 1 0) 0)
(expect (and 0 2) 0)
(expect (and 0.0 2) 0.0)
(expect (and 1 _) _)
(expect (and 1 "foo") "foo")

; or
(expect (or t t) t)
(expect (or t f) t)
(expect (or f t) t)
(expect (or f f) f)

(expect (or 1 2) 1)
(expect (or 0 2) 2)
(expect (or 0.0 2) 2)
(expect (or _ 2) 2)
(expect (or f "foo") "foo")

; not
(expect (not t) f)
(expect (not f) t)
(expect (not 0) t)
(expect (not 0.0) t)
(expect (not _) t)
(expect (not 1) f)
(expect (not -1) f)
(expect (not "foo") f)
(expect (not []) f)


; eq — integers
; ----------------------------------------------------------------------

(expect (eq 5 5) t)
(expect (eq 5 6) f)
(expect (eq -5 -5) t)
(expect (eq -5 5) f)
(expect (eq 0 0) t)


; eq — integer / float
; ----------------------------------------------------------------------

(expect (eq 5 5.0) t)
(expect (eq 5.0 5) t)
(expect (eq 5 5.1) f)
(expect (eq 5.1 5) f)

(expect (eq 0 0.0) t)
(expect (eq -5 -5.0) t)

; Important: exact numeric comparison, no epsilon
(expect (eq 0.1 0.10000000000000001) t)
(expect (eq 0.1 0.10000000000000002) f)


; eq — floats
; ----------------------------------------------------------------------

(expect (eq 1.0 1.0) t)
(expect (eq 1.0 2.0) f)
(expect (eq -1.5 -1.5) t)
(expect (eq -1.5 1.5) f)

; IEEE-754 special values
(expect ($ "%t:%s" NaN NaN) "float:NaN")
(expect ($ "%t:%s" Inf Inf) "float:Inf")
(expect ($ "%t:%s" -Inf -Inf) "float:-Inf")
(expect (eq NaN NaN) f)
(expect (eq Inf Inf) t)
(expect (eq -Inf -Inf) t)
(expect (eq 5 Inf) f)
(expect (eq 5.0 Inf) f)
(expect (lt 1.0 Inf) t)
(expect (lt -Inf 1.0) t)
(expect (gt -Inf 1.0) f)
(expect (lt NaN 1.0) f)
(expect (gt NaN 1.0) f)

; IEEE-754 signed zero
(expect (eq 0.0 -0.0) t)


; ne
; ----------------------------------------------------------------------

(expect (ne 5 6) t)
(expect (ne 5 5) f)
(expect (ne 5 5.0) f)
(expect (ne 5 5.1) t)

(expect (ne 1.0 2.0) t)
(expect (ne 1.0 1.0) f)

(expect (ne NaN NaN) t)
(expect (ne 0.0 -0.0) f)


; lt / gt
; ----------------------------------------------------------------------

(expect (lt 2 3) t)
(expect (lt 3 2) f)
(expect (lt 3 3) f)

(expect (lt 2 3.0) t)
(expect (lt 3.0 2) f)
(expect (lt 3.0 3.0) f)

(expect (gt 3 2) t)
(expect (gt 2 3) f)
(expect (gt 3 3) f)

(expect (gt 3 2.0) t)
(expect (gt 2.0 3) f)


; le / ge
; ----------------------------------------------------------------------

(expect (le 2 3) t)
(expect (le 3 3) t)
(expect (le 3 2) f)

(expect (le 2 3.0) t)
(expect (le 3.0 3) t)
(expect (le 3.0 2) f)

(expect (ge 3 2) t)
(expect (ge 3 3) t)
(expect (ge 2 3) f)

(expect (ge 3 2.0) t)
(expect (ge 3.0 3) t)
(expect (ge 2.0 3) f)


; Short-circuit evaluation
; ----------------------------------------------------------------------

(expect (and f (div 1 0)) f)
(expect (or t (div 1 0)) t)


; if
; ----------------------------------------------------------------------

(expect (if t 42) 42)
(expect (if f 42) _)

(expect (if t 42 21) 42)
(expect (if f 42 21) 21)

(expect (if 0 "zero" "non-zero") "non-zero")
(expect (if 0.0 "zero" "non-zero") "non-zero")
(expect (if _ "value" "null") "null")
(expect (if "foo" "yes" "no") "yes")

; match
; ----------------------------------------------------------------------

(expect (match (eq 1 2) "no" (eq 1 1) "yes" t "default") "yes")
(expect (match (eq 1 2) "no" (gt 1 2) "greater" t "default") "default")
(expect (match (eq 5 5) 42 t 0) 42)
(expect (match (eq 5 6) 42 t 0) 0)

; Math
; Arithmetic

; int64
(expect ($ "%d" (add 2 3)) "5")
(expect ($ "%d" (sub 10 3)) "7")
(expect ($ "%d" (mul 6 7)) "42")
(expect ($ "%d" (div 10 2)) "5")
(expect ($ "%d" (div 7 2)) "3")
(expect ($ "%d" (mod -7 2)) "-1")
(expect ($ "%d" (pow 2 10)) "1024")

; f64
(expect ($ "%f" (div 7.0 2.0)) "3.5")
(expect ($ "%f" (add 1.5 2.0)) "3.5")
(expect ($ "%f" (sub 5.5 2.0)) "3.5")
(expect ($ "%f" (mul 1.75 2.0)) "3.5")
(expect ($ "%f" (div 7.0 2.0)) "3.5")
(expect ($ "%f" (pow 1.5 2.0)) "2.25")
(expect ($ "%f" (add -1.5 2.0)) "0.5")
(expect ($ "%f" (sub -1.5 2.0)) "-3.5")
(expect ($ "%f" (mul -1.5 2.0)) "-3")
(expect ($ "%f" (div -7.0 2.0)) "-3.5")

; Type promotion
(expect ($ "%t:%s" (add 1 2.5) (add 1 2.5)) "float:3.5")
(expect ($ "%t:%s" (sub 5 1.5) (sub 5 1.5)) "float:3.5")
(expect ($ "%t:%s" (mul 7 0.5) (mul 7 0.5)) "float:3.5")
(expect ($ "%t:%s" (div 7 2.0) (div 7 2.0)) "float:3.5")
(expect ($ "%t:%s" (div 7 2)   (div 7 2))   "int:3")
(expect ($ "%t:%s" (div 7 2.0) (div 7 2.0)) "float:3.5")

; Bitwise operation
(expect ($ "%d" (bit-and 0b110 0b101)) "4")
(expect ($ "%d" (bit-or 0b110 0b101)) "7")
(expect ($ "%d" (bit-xor 0b110 0b101)) "3")
(expect ($ "%d" (bit-not 0b101)) "-6")
(expect ($ "%d" (bit-shl 1 4)) "16")
(expect ($ "%d" (bit-shr 16 2)) "4")

; let/set (Shadowing)
(let x 1)
(let x 2)
(expect x 2)


(let x 1)
(
  (let x 2)
  (expect x 2)
)
(expect x 1)


(let x 1)
(
  (let x 2)
  (set x 3)
  (expect x 3)
)
(expect x 1)


(let x 1)
(
  (let y 2)
  (set x 3)
)
(expect x 3)

; Loop / break / continue
; ----------------------------------------------------------------------

; loop with break
(let x 5)
(let result
  (loop
    (if (eq x 0) (break "fini"))
    (set x (sub x 1))))
(expect result "fini")
(expect x 0)

; break with a value
(expect
  (loop
    (break 42))
  42)

(expect
  (loop
    (break "done"))
  "done")

(expect
  (loop
    (break t))
  t)


; break without a value
(expect
  (loop
    (break))
  _)


; break from inside a conditional
(expect
  (loop
    (if t
        (break 42)))
  42)

(expect
  (loop
    (if f
        (break 42))
    (break 99))
  99)

; loop body executes repeatedly
(let x 0)
(let result
  (loop
    (if (eq x 5)
        (break x))
    (set x (add x 1))))
(expect result 5)
(expect x 5)


; continue skips the rest of the current iteration
(let x 0)
(let count 0)
(let result
  (loop
    (if (eq x 5)
        (break count))
    (set x (add x 1))
    (if (eq x 3)
        (continue))
    (set count (add count 1))))
(expect result 4)
(expect count 4)
(expect x 5)


; continue does not terminate the loop
(let x 0)
(let result
  (loop
    (set x (add x 1))
    (if (lt x 5)
        (continue))
    (break x)))
(expect result 5)


; continue inside a conditional
(let x 0)
(let result
  (loop
    (set x (add x 1))
    (if (eq x 3)
        (continue))
    (if (eq x 5)
        (break x))))
(expect result 5)


; ----------------------------------------------------------------------
; Scope
; ----------------------------------------------------------------------

; () opens a scope
(let x 10)
(
  (let x 20)
  (expect x 20)
)
(expect x 10)


; Loop body scope does not leak
(let x 10)
(
  (loop
    (let y 42)
    (break y))
)
(expect x 10)


; variable created in scope is unavailable outside
(
  (let x 42)
  (expect x 42)
)
; expect unresolved/unknown variable outside the scope


; Mutation of an outer variable survives the scope
(let x 10)
(
  (set x 42)
)
(expect x 42)


; Loop can mutate an outer variable
(let x 0)
(
  (loop
    (if (eq x 5)
        (break))
    (set x (add x 1)))
)
(expect x 5)


; Variables declared inside loop scope are local
(
  (loop
    (let x 42)
    (break x))
)
; x must not exist here


; ----------------------------------------------------------------------
; Nested scopes
; ----------------------------------------------------------------------

(let x 1)
(
  (let x 2)
  (
    (let x 3)
    (expect x 3)
  )
  (expect x 2)
)
(expect x 1)


; ----------------------------------------------------------------------
; Nested loops
; ----------------------------------------------------------------------

(let outer 0)
(let inner 0)

(let result
  (loop
    (set outer (add outer 1))
    (set inner 0)

    (loop
      (set inner (add inner 1))
      (if (eq inner 3)
          (break "inner")))

    (if (eq outer 2)
        (break "outer"))))

(expect result "outer")
(expect outer 2)
(expect inner 3)

; break only exits the innermost loop
(let outer 0)
(let inner 0)

(let result
  (loop
    (set outer (add outer 1))
    (set inner 0)

    (loop
      (set inner (add inner 1))
      (break "inner"))

    (if (eq outer 3)
        (break "outer"))))

(expect result "outer")
(expect outer 3)
(expect inner 1)


; ============================================================
; Variadic builtins
; ============================================================

; ------------------------------------------------------------
; ADD — 0+
; ------------------------------------------------------------

(expect (add) 0 "add: empty")
(expect (add 1) 1 "add: one")
(expect (add 1 2) 3 "add: two")
(expect (add 1 2 3 4) 10 "add: many")

(expect (add -1 2 -3 4) 2 "add: negative integers")

(expect ($ "%t:%s" (add 1 2.5) (add 1 2.5))
        "float:3.5"
        "add: integer/float promotion")

; Overflow during fold
; (expect-error (add 9223372036854775807 1 2) IntegerOverflow)


; ------------------------------------------------------------
; MUL — 0+
; ------------------------------------------------------------

(expect (mul) 1 "mul: empty")
(expect (mul 2) 2 "mul: one")
(expect (mul 2 3) 6 "mul: two")
(expect (mul 2 3 4) 24 "mul: many")

(expect (mul -2 3 -4) 24 "mul: negative integers")

(expect ($ "%t:%s" (mul 2 2.5) (mul 2 2.5))
        "float:5"
        "mul: integer/float promotion")


; ------------------------------------------------------------
; SUB — 1+
; ------------------------------------------------------------

(expect (sub 5) -5 "sub: unary")
(expect (sub 10 3) 7 "sub: two")
(expect (sub 10 3 2) 5 "sub: left fold")
(expect (sub 20 5 3 2) 10 "sub: many")

(expect ($ "%t:%s" (sub 10 2.5) (sub 10 2.5))
        "float:7.5"
        "sub: integer/float promotion")


; ------------------------------------------------------------
; DIV — 1+
; ------------------------------------------------------------

(expect ($ "%t:%s" (div 10) (div 10))
        "float:0.1"
        "div: unary reciprocal")

(expect ($ "%t:%s" (div 2.5) (div 2.5))
        "float:0.4"
        "div: unary reciprocal float")

(expect ($ "%t:%s" (div 10 2) (div 10 2))
        "int:5"
        "div: integer division")

(expect ($ "%t:%s" (div 10 2 5) (div 10 2 5))
        "int:1"
        "div: left fold")

(expect ($ "%t:%s" (div 10 2 4) (div 10 2 4))
        "int:1"
        "div: promotion during fold")


; ------------------------------------------------------------
; EQ — 0+
; ------------------------------------------------------------

(expect (eq) t "eq: empty")
(expect (eq 1) t "eq: one")
(expect (eq 1 1) t "eq: two")
(expect (eq 1 1 1) t "eq: all equal")
(expect (eq 1 1 2) f "eq: one differs")

(expect (eq 5 5.0) t "eq: integer/float")

(expect (eq [1 2] [1 2]) t "eq: arrays")
(expect (eq [1 2] [2 1]) f "eq: array order")

(expect (eq {"a":1 "b":2} {"b":2 "a":1})
        t
        "eq: struct key order")


; ------------------------------------------------------------
; NE — 0+
; ------------------------------------------------------------

(expect (ne) t "ne: empty")
(expect (ne 1) t "ne: one")
(expect (ne 1 2) t "ne: different")
(expect (ne 1 2 3) t "ne: all different")
(expect (ne 1 2 1) f "ne: repeated value")


; ------------------------------------------------------------
; LT — chained
; ------------------------------------------------------------

(expect (lt) t "lt: empty")
(expect (lt 1) t "lt: one")
(expect (lt 1 2) t "lt: two")
(expect (lt 1 2 3) t "lt: increasing")
(expect (lt 1 2 3 4 5) t "lt: increasing many")
(expect (lt 1 3 2) f "lt: not increasing")
(expect (lt 1 2 2) f "lt: equal adjacent")

; Mixed numeric types
(expect (lt 1 2.0 3) t "lt: mixed numeric types")


; ------------------------------------------------------------
; GT — chained
; ------------------------------------------------------------

(expect (gt) t "gt: empty")
(expect (gt 5) t "gt: one")
(expect (gt 3 2) t "gt: two")
(expect (gt 5 4 3 2 1) t "gt: decreasing")
(expect (gt 5 4 4) f "gt: equal adjacent")
(expect (gt 5 3 4) f "gt: not decreasing")


; ------------------------------------------------------------
; LE — chained
; ------------------------------------------------------------

(expect (le) t "le: empty")
(expect (le 1) t "le: one")
(expect (le 1 2) t "le: increasing")
(expect (le 1 2 2 3) t "le: equal allowed")
(expect (le 1 2 1) f "le: violation")


; ------------------------------------------------------------
; GE — chained
; ------------------------------------------------------------

(expect (ge) t "ge: empty")
(expect (ge 3) t "ge: one")
(expect (ge 3 2 2 1) t "ge: equal allowed")
(expect (ge 3 2 4) f "ge: violation")


; ------------------------------------------------------------
; NaN / Infinity
; ------------------------------------------------------------

(expect (eq Inf Inf) t "eq: Inf")
(expect (eq -Inf -Inf) t "eq: -Inf")
(expect (eq NaN NaN) f "eq: NaN")

(expect (lt 1.0 Inf) t "lt: Inf")
(expect (gt -Inf 1.0) f "gt: -Inf")

(expect (lt NaN 1.0) f "lt: NaN")
(expect (gt NaN 1.0) f "gt: NaN")
(expect (le NaN 1.0) f "le: NaN")
(expect (ge NaN 1.0) f "ge: NaN")


; ------------------------------------------------------------
; MOD — exactly 2
; ------------------------------------------------------------

(expect (mod 7 2) 1 "mod")

; Expected errors:
; (expect-error (mod) ArityError)
; (expect-error (mod 7) ArityError)
; (expect-error (mod 7 2 1) ArityError)


; ------------------------------------------------------------
; POW — exactly 2
; ------------------------------------------------------------

(expect (pow 2 10) 1024 "pow")

; Expected errors:
; (expect-error (pow) ArityError)
; (expect-error (pow 2) ArityError)
; (expect-error (pow 2 3 4) ArityError)


; ------------------------------------------------------------
; BIT-AND — 0+
; ------------------------------------------------------------

(expect (bit-and) -1 "bit-and: identity")
(expect (bit-and 7) 7 "bit-and: one")
(expect (bit-and 7 3) 3 "bit-and: two")
(expect (bit-and 7 3 1) 1 "bit-and: many")


; ------------------------------------------------------------
; BIT-OR — 0+
; ------------------------------------------------------------

(expect (bit-or) 0 "bit-or: identity")
(expect (bit-or 7) 7 "bit-or: one")
(expect (bit-or 1 2 4) 7 "bit-or: many")


; ------------------------------------------------------------
; BIT-XOR — 0+
; ------------------------------------------------------------

(expect (bit-xor) 0 "bit-xor: identity")
(expect (bit-xor 7) 7 "bit-xor: one")
(expect (bit-xor 7 3 1) 5 "bit-xor: many")
(expect (bit-xor 7 7) 0 "bit-xor: cancellation")


; ------------------------------------------------------------
; BIT-NOT — exactly 1
; ------------------------------------------------------------

(expect (bit-not 0) -1 "bit-not")
(expect (bit-not 5) -6 "bit-not")
(expect (bit-not -1) 0 "bit-not: negative")

; Expected errors:
; (expect-error (bit-not) ArityError)
; (expect-error (bit-not 1 2) ArityError)


; ============================================================
; User-defined functions remain fixed-arity
; ============================================================

(let sum-two
  (fn (a b)
    (add a b)))

(expect (sum-two 1 2) 3 "user fn: normal call")

; Expected errors:
; (expect-error (sum-two 1) ArityError)
; (expect-error (sum-two 1 2 3) ArityError)


; ============================================================
; Nested / late errors during variadic folds
; ============================================================

; These are particularly important because the error happens
; after earlier operands have already been processed.

; (expect-error
;   (add 1 2 9223372036854775807 1)
;   IntegerOverflow)

; (expect-error
;   (mul 1 2 4611686018427387904 3)
;   IntegerOverflow)

; (expect-error
;   (div 100 5 0)
;   DivisionByZero)

; (expect-error
;   (mod 10 0)
;   DivisionByZero)


; ============================================================
; Evaluation happens exactly once and left-to-right
; ============================================================

(let x 0)
(let record
  (fn (value)
    ((set x value) value)))

(add
  (record 1)
  (record 2)
  (record 3))

(expect x 3 "variadic args: left-to-right")


; ============================================================
; Existing 2-argument behavior remains unchanged
; ============================================================

(expect (add 2 3) 5 "add: existing")
(expect (sub 10 3) 7 "sub: existing")
(expect (mul 6 7) 42 "mul: existing")

(expect ($ "%t:%s" (div 10 2) (div 10 2))
        "int:5"
        "div: existing")

(expect ($ "%t:%s" (div 7 2) (div 7 2))
        "int:3"
        "div: existing integer division")

(expect (mod -7 2) -1 "mod: existing")
(expect (pow 2 10) 1024 "pow: existing")

(expect (eq 5 5.0) t "eq: existing mixed numeric")
(expect (ne 5 6) t "ne: existing")
(expect (lt 2 3) t "lt: existing")
(expect (gt 3 2) t "gt: existing")
(expect (le 3 3) t "le: existing")
(expect (ge 3 3) t "ge: existing")


; REGEX

(let match "old")
(expect (~ "xxx" "hello" match) f)
(expect match "old")
