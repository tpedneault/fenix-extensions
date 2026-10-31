; What folds in Ada, and the scopes SPC c s and the breadcrumbs list:
; for tree-sitter-ada 0.9.2. The grammar's own folds (MIT), with the
; units and types first, then tasks, protected objects, records and the
; statements that hold others.

[
  (package_declaration)
  (generic_package_declaration)
  (package_body)
  (subprogram_body)
  (expression_function_declaration)
  (task_type_declaration)
  (single_task_declaration)
  (task_body)
  (protected_type_declaration)
  (single_protected_declaration)
  (protected_body)
  (entry_body)
  (record_definition)
  (block_statement)
  (if_statement)
  (case_statement)
  (loop_statement)
  (select_alternative)
  (accept_statement)
  (exception_handler)
  (gnatprep_declarative_if_statement)
  (gnatprep_if_statement)
] @fold
