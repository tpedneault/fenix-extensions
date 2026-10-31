; Where Enter indents a level more in Ada: inside a package, a body's
; declarations and statements, a record, a task or protected object, a
; statement that holds others, a case's alternative, an exception
; handler, a generic's formals and a call's arguments. For
; tree-sitter-ada 0.9.2, whose bodies have no block node of their own:
; the unit or statement that starts on the line holds them. The
; alternatives of a case or a select, and the handlers after
; `exception`, are a level in, as GNAT's style (-gnaty3) puts them.

[
  (package_declaration)
  (package_body)
  (generic_formal_part)
  (subprogram_body)
  (subprogram_declaration)
  (task_definition)
  (task_body)
  (protected_definition)
  (protected_body)
  (entry_body)
  (record_definition)
  (block_statement)
  (if_statement)
  (elsif_statement_item)
  (case_statement)
  (case_statement_alternative)
  (loop_statement)
  (selective_accept)
  (select_alternative)
  (accept_statement)
  (handled_sequence_of_statements)
  (exception_handler)
  (actual_parameter_part)
  (formal_part)
  (aspect_specification)
] @indent
