
(let complex-struct {"name":"Yvan","age":56,"address":"1 rue de paris","city":"Paris","contact":{"gsm":"0102030405","fax":"0102030406"},"score":[1, 2, [21, [211,212], 22, 23], 3]})



(expect (ne) t "ne: empty")
(expect (ne 1) t "ne: one")
(expect (ne 1 2) t "ne: different")
(expect (ne 1 2 3) t "ne: all different")
(expect (ne 1 2 1) f "ne: repeated value")
