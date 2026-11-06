; What folds in OCaml, and the scopes SPC c s and the breadcrumbs list:
; for tree-sitter-ocaml 0.24.2, its implementations and its interfaces.
; The grammar has no folds.scm of its own; these are the pack's, the
; definitions a scope is named by first -- a `let`, at the top or in an
; expression, a type, a module and its signature, a class.

(value_definition) @fold
(type_definition) @fold
(exception_definition) @fold
(external) @fold
(module_definition) @fold
(module_type_definition) @fold
(class_definition) @fold
(class_type_definition) @fold
(method_definition) @fold
(value_specification) @fold
(structure) @fold
(signature) @fold
(object_expression) @fold
(match_expression) @fold
(function_expression) @fold
(fun_expression) @fold
(try_expression) @fold
(if_expression) @fold
(for_expression) @fold
(while_expression) @fold
(record_expression) @fold
(list_expression) @fold
(array_expression) @fold
(comment) @fold
