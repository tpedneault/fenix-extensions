; What folds in Zig, and the scopes SPC c s and the breadcrumbs list: for
; tree-sitter-zig 1.1.2. From the grammar's own folds.scm (MIT), with the
; declarations a scope is named by first.

(function_declaration) @fold
(test_declaration) @fold
(struct_declaration) @fold
(enum_declaration) @fold
(union_declaration) @fold
(opaque_declaration) @fold
(error_set_declaration) @fold
(switch_expression) @fold
(initializer_list) @fold
(asm_expression) @fold
(multiline_string) @fold
(if_statement) @fold
(while_statement) @fold
(for_statement) @fold
