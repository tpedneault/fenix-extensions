; Where Enter indents a level more in Nix: inside a set, a list,
; parentheses or an argument pattern, after `let` and `in`'s bindings, and
; in a multi-line string. For tree-sitter-nix 0.3.0.

[
  (attrset_expression)
  (rec_attrset_expression)
  (list_expression)
  (let_expression)
  (parenthesized_expression)
  (formals)
  (indented_string_expression)
] @indent
