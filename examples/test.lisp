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
; Array
(expect ($ "%t:%s" [1 2 3] [1 2 3]) "array:[1 2 3]")
;Struct
(expect ($ "%t:%s" {"name":"Ada"} {"name":"Ada"}) "struct:{name:\"Ada\"}")
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
