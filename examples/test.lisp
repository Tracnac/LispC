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
(expect ($ "%t:%s" {"name":"Ada"} {"name":"Ada"}) "struct:{\"name\":Ada}")
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

; and — truthiness
(expect (and 1 2) t)
(expect (and 1 0) f)
(expect (and 1 0.0) f)
(expect (and 1 _) f)
(expect (and 1 "foo") t)
(expect (and 1 []) t)

; or
(expect (or t t) t)
(expect (or t f) t)
(expect (or f t) t)
(expect (or f f) f)

; or — truthiness
(expect (or 0 1) t)
(expect (or 0 0.0) f)
(expect (or _ f) f)
(expect (or 0 "foo") t)
(expect (or 0 []) t)

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
(expect (if _ "null" "value") "null")
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
