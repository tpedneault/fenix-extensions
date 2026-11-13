; Where Enter indents a level more in Terraform and HCL: inside a block,
; an object or a list, a call's arguments or a `for`. For tree-sitter-hcl
; 1.2.0.

[
  (block)
  (object)
  (tuple)
  (function_call)
  (for_tuple_expr)
  (for_object_expr)
] @indent
