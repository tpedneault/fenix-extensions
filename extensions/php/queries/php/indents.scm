; Where Enter indents a level more in PHP: inside a block, a class's or
; an enum's body, a call's arguments, a function's parameters, an array,
; a match or a switch. For tree-sitter-php 0.24.2.

[
  (compound_statement)
  (declaration_list)
  (enum_declaration_list)
  (arguments)
  (formal_parameters)
  (array_creation_expression)
  (match_block)
  (switch_block)
  (property_hook_list)
] @indent
