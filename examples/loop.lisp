; Array indexing and slicing
      ; [1]      first element
      ; [[1 2]]  Element 1 and 2
      ; [..5]    from 1 to 5
      ; [2..5]   from 2 to 5 (inclusive)
      ; [5..]    from 5 to end
      ; [-1]     last element
      ; [-5..-2] 5th-to-last through 2nd-to-last

(let complex-struct {"name":"Yvan","age":56,"address":"1 rue de paris","city":"Paris","contact":{"gsm":"0102030405","fax":"0102030406"},"score":[1, 2, [21, [211,212], 22, 23], 3]})

(io/write 1 ($ "%s\n" complex-struct))
(io/write 1 ($ "%s\n" complex-struct.score[2])) ; => 2
(io/write 1 ($ "%s\n" complex-struct.score[3])) ; => [21,[211,212],22,23]
(io/write 1 ($ "%s\n" complex-struct.score[3][2][2])) ; => 212
(io/write 1 ($ "%s\n" complex-struct.score[3][[1 3]])) ; => [21 22]
(io/write 1 ($ "%s\n" complex-struct.score[3][2][[1 2]])) ; => [211 212]
