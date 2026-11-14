; What folds in Nix, and the scopes SPC c s and the breadcrumbs list: sets
; and lists that span lines, `let ... in`, strings of several lines, and
; each binding that holds one -- `services.orbit = { ... };`. For
; tree-sitter-nix 0.3.0.

(attrset_expression) @fold
(rec_attrset_expression) @fold
(list_expression) @fold
(let_expression) @fold
(indented_string_expression) @fold
(formals) @fold
(parenthesized_expression) @fold
(if_expression) @fold
(binding expression: [(attrset_expression) (rec_attrset_expression) (list_expression) (function_expression) (apply_expression) (indented_string_expression)]) @fold
