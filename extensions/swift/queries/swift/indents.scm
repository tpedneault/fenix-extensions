; Where Enter indents a level more in Swift: inside a type's or a
; function's body, a block, a case, a closure, a call's arguments, a
; literal. For tree-sitter-swift 0.7.1, whose blocks have no node of
; their own: the statement that starts on the line holds them. A
; switch's cases stay at its level, as swift-format puts them.

[
  (class_body)
  (enum_class_body)
  (protocol_body)
  (function_body)
  (computed_property)
  (computed_getter)
  (computed_setter)
  (willset_didset_block)
  (if_statement)
  (guard_statement)
  (for_statement)
  (while_statement)
  (repeat_while_statement)
  (do_statement)
  (catch_block)
  (switch_entry)
  (lambda_literal)
  (value_arguments)
  (array_literal)
  (dictionary_literal)
] @indent
