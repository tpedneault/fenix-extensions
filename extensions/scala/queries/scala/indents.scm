; Where Enter indents a level more in Scala, for tree-sitter-scala 0.24.0: inside
; braces, parentheses and an enum's or template's body, and right after
; what opens an indented block in Scala 3 -- `=`, `:`, `=>`, `with`,
; `then`, `else`, `do`, `try` and `finally` at the end of a line.

[
  (template_body)
  (with_template_body)
  (enum_body)
  (block)
  (case_block)
  (arguments)
  (parameters)
  (class_parameters)
  (type_parameters)
  (type_arguments)
  (namespace_selectors)
  (tuple_expression)
] @indent

[
  "="
  ":"
  "=>"
  "?=>"
  "with"
  "then"
  "else"
  "do"
  "try"
  "finally"
] @indent
