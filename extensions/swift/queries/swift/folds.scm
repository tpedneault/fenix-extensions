; What folds in Swift, and the scopes SPC c s and the breadcrumbs list:
; for tree-sitter-swift 0.7.1 (alex-pinkus). Its class_declaration is
; classes, structs, actors, enums and extensions alike.

(class_declaration) @fold
(protocol_declaration) @fold
(function_declaration) @fold
(init_declaration) @fold
(deinit_declaration) @fold
(subscript_declaration) @fold
(computed_property) @fold
(computed_getter) @fold
(computed_setter) @fold
(willset_didset_block) @fold
(switch_statement) @fold
(do_statement) @fold
(lambda_literal) @fold
(multiline_comment) @fold
