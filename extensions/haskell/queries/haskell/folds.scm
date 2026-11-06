; What folds in Haskell, and the scopes SPC c s and the breadcrumbs list:
; for tree-sitter-haskell 0.23.1. The grammar has no folds.scm of its
; own; these are the pack's, the declarations a scope is named by first.

(decl/function) @fold
(decl/bind) @fold
(data_type) @fold
(newtype) @fold
(class) @fold
(instance) @fold
(type_family) @fold
(data_family) @fold
(type_synomym) @fold
(header) @fold
(imports) @fold
(local_binds) @fold
(do) @fold
(case) @fold
(lambda_case) @fold
(multi_way_if) @fold
(let_in) @fold
(conditional) @fold
(expression/record) @fold
(expression/list) @fold
(comment) @fold
(haddock) @fold
