(use "io")
(let a 5)

(loop
  (match
    (eq a 1)(io.write 1 "Case1\n")
    (eq a 1)(io.write 1 "Case2\n")
    t (break)))
