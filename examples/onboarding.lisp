; A small interactive onboarding script.
; Run: cargo run -- examples/onboarding.lisp

(io/write 1 "Small Lisp onboarding\n")
(io/write 1 "What is your name?\n")
(let name (io/read 0))

; Regex matching can validate a non-empty answer and bind its captures.
(match
  (~ "^(.+)$" name name-match)
  (
    (let profile {"name":name "visits":0 "roles":["reader" "writer"]})

    ; A function receives an explicit alias, so it mutates the struct field.
    (let register-visit (fn (count) (set count (add count 1))))
    (register-visit ^profile.visits)

    (let greeting (fn (person)
      ($ "Welcome, %s. This is visit #%d.\n" person.name person.visits)))

    (io/write 1 (greeting profile))
    (io/write 1 ($ "Default role: %s\n" profile.roles[1]))

    ; A loop can return a value through break.
    (let seconds 3)
    (let status
      (loop
        (if (eq seconds 0) (break "ready"))
        ((io/write 1 ($ "Starting in %d...\n" seconds))
         (set seconds (sub seconds 1)))))
    (io/write 1 ($ "Service status: %s\n" status))
  )
  t
  (io/write 1 "A name is required; please run the program again.\n")
)
