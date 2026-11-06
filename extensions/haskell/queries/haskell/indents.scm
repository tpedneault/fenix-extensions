; Where Enter indents a level more in Haskell, for tree-sitter-haskell
; 0.23.1: after a line that ends in what opens a block -- a `where`, an
; equation's `=` or a case's `->`, `do`, `of`, `then`, `else`, `let`, or
; a bracket -- that is: one of these that ends where the cursor is. Fenix indents in a
; node that starts on the cursor's line and ends at it or after, so a
; whole equation as one (`f x = x`, on a line) would indent after each
; one-liner; its `=` ends before the cursor unless it's the line's last.

(class "where" @indent)
(instance "where" @indent)
(data_type ["=" "where"] @indent)
(function "where" @indent)
(bind "where" @indent)
(match ["=" "->"] @indent)
(lambda "->" @indent)
(do ["do" "mdo"] @indent)
(case "of" @indent)
(conditional ["then" "else"] @indent)
(let "let" @indent)

["(" "[" "{"] @indent
