; Where Enter indents a level more in Kotlin: inside a block, a body, a
; call's arguments, a `when`, a lambda. For tree-sitter-kotlin 0.3.8.

[
  (class_body)
  (enum_class_body)
  (function_body)
  (control_structure_body)
  (lambda_literal)
  (when_expression)
  (value_arguments)
  (function_value_parameters)
  (primary_constructor)
  (collection_literal)
  (try_expression)
  (catch_block)
  (finally_block)
] @indent
