; Where Enter indents a level more in OCaml, for tree-sitter-ocaml 0.24.2:
; after a line that ends in what opens a block -- a binding's `=`, a
; case's or a `fun`'s `->`, `struct`, `sig`, `object`, `begin`, `do`,
; `then`, `else`, or a bracket. Fenix indents in a node that starts on the
; cursor's line and ends at it or after, so a whole `let` as one would
; indent after each one-liner; its `=` ends before the cursor unless it's
; the line's last. A `match ... with` isn't: its cases go below it, at
; its own indent, as ocamlformat puts them.

(let_binding "=" @indent)
(type_binding "=" @indent)
(method_definition "=" @indent)
(match_case "->" @indent)
(fun_expression "->" @indent)

[
  "struct"
  "sig"
  "object"
  "begin"
  "do"
  "then"
  "else"
  "("
  "["
  "{"
  "[|"
] @indent
