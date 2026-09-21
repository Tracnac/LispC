(let outer 0)
(let inner 0)

(let result
  (loop
    (set outer (add outer 1))

    (loop
      (set inner (add inner 1))
      (if (eq inner 3)
          (break "inner")))

    (if (eq outer 2)
        (break "outer"))))

(expect result "outer")
(expect outer 2)
(expect inner 3)
