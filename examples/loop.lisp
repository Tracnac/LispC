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
