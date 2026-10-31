; Where Enter indents a level more in Zig: inside a block, a container's
; body, a switch, an initializer, a call's arguments or a function's
; parameters. For tree-sitter-zig 1.1.2.

[
  (block)
  (struct_declaration)
  (enum_declaration)
  (union_declaration)
  (opaque_declaration)
  (error_set_declaration)
  (switch_expression)
  (initializer_list)
  (call_expression)
  (parameters)
] @indent
