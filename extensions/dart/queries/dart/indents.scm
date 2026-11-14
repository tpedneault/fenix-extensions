; Where Enter indents a level more in Dart: inside a type's or a
; function's body, a block, a switch, a literal, a call's arguments or a
; parameter list. For tree-sitter-dart (UserNobody14's).

[
  (class_body)
  (enum_body)
  (extension_body)
  (block)
  (switch_block)
  (list_literal)
  (set_or_map_literal)
  (arguments)
  (formal_parameter_list)
  (record_literal)
] @indent
