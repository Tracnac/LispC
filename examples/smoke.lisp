; scopes and closures
(let x 10)
((let x 20) (io/write 1 ($ "%d\n" x)))
(io/write 1 ($ "%d\n" x))
(let counter 0)
(let inc (fn () (set counter (add counter 1))))
(inc) (inc)
(io/write 1 ($ "%d\n" counter))

; recursion
(let fact (fn (n) (if (eq n 0) 1 (mul n (fact (sub n 1))))))(io/write 1 ($ "%d\n" (fact 5)))

; aliases and containers
(let user {"name":"Yvan" "tags":["dev" "lisp"]})
(let name (^ user.name))
(set name "Nouveau")
(io/write 1 ($ "%s\n" user.name))
(let nums [1 2 3])
(let push-zero (fn (a) (set a[1] 0)))
(push-zero ^nums)
(io/write 1 ($ "%d\n" nums[1]))

; arithmetic, loop, match, regex
(io/write 1 ($ "%f\n" (div 7 2)))
(let i 3)
(loop (if (eq i 0) (break "done")) ((io/write 1 ($ "%d\n" i)) (set i (sub i 1))))
(match (eq x 0)(io/write 1 "x=0\n") t (io/write 1 "Default\n"))
(io/write 1 ($ "%s\n" (~ "^a.+z$" "abz" regex-match)))
