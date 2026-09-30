; IO refactor
; Goal simplify io via builtin easy to use.
; Should support most uri that make sense in Lisp.
  ; file:
  ; stdin:
  ; stdout:
  ; stderr:
  ; tcp:
  ; udp:
  ; unix:
  ; http:

; URI	Purpose	Status
; file:	Filesystem files	Implemented / current design
; stdin:	Standard input	Discussed
; stdout:	Standard output	Discussed
; stderr:	Standard error	Discussed
; tcp:	TCP connection	Discussed / proposed
; udp:	UDP connection	Discussed / proposed
; unix:	Unix-domain socket	Discussed / proposed
; http:	HTTP resource	Discussed, but kept outside io.open

(let fib (fn (n) (if (lt n 2) n (add (fib (sub n 1))(fib (sub n 2))))))
