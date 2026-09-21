(let a [])
(let b a[..])

(let string "Hello the world")
(let substr string[..])

(~ "^Hello(.*)$" string myvar) ; => ["Hello the world" "the world"]
;                                      ^ match           ^groups
(io/write 1 myvar[2]) ; => the world
