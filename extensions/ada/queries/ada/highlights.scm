; Ada's highlighting, for tree-sitter-ada 0.9.2 (Emmanuel Briot's).
;
; From that grammar's own queries/highlights.scm (MIT, Copyright (c) 2023
; Emmanuel Briot), with Fenix's capture names (no `@spell`, `@error` or
; `@exception`, which it doesn't colour), the aspects and pragmas SPARK
; lives in (`Pre`, `Post`, `Global`, `Loop_Invariant`) as attributes, and
; attributes (`'Old`, `'Range`) as properties. Later patterns win where
; two match the same text. Ada has no `true` or `false` of its own: they
; are Boolean's literals, read as names.

[
  "abort"
  "abs"
  "abstract"
  "accept"
  "access"
  "all"
  "array"
  "at"
  "begin"
  "declare"
  "delay"
  "delta"
  "digits"
  "do"
  "end"
  "entry"
  "exit"
  "finally"
  "generic"
  "goto"
  "interface"
  "is"
  "limited"
  "null"
  "of"
  "others"
  "out"
  "pragma"
  "private"
  "range"
  "requeue"
  "synchronized"
  "tagged"
  "task"
  "terminate"
  "until"
  "when"
] @keyword

[
  "aliased"
  "constant"
  "renames"
] @storageclass

[
  "mod"
  "new"
  "protected"
  "record"
  "subtype"
  "type"
] @keyword.type

[
  "with"
  "use"
] @include

[
  "body"
  "function"
  "overriding"
  "procedure"
  "package"
  "separate"
] @keyword.function

[
  "and"
  "in"
  "not"
  "or"
  "xor"
  "rem"
] @keyword.operator

[
  "while"
  "loop"
  "for"
  "parallel"
  "reverse"
  "some"
] @repeat

"return" @keyword.return

[
  "case"
  "if"
  "else"
  "then"
  "elsif"
  "select"
] @conditional

[
  "exception"
  "raise"
] @keyword.exception

[
  (relational_operator)
  (binary_adding_operator)
  (unary_adding_operator)
  (multiplying_operator)
  ":="
  "=>"
  ".."
  "**"
  "<>"
] @operator

[
  ";"
  ","
  ":"
  "."
] @punctuation.delimiter

[
  "("
  ")"
] @punctuation.bracket

(comment) @comment
(string_literal) @string
(character_literal) @string
(numeric_literal) @number

;; Names: a subprogram's, a package's, a type's.

(procedure_specification name: (_) @function)
(function_specification name: (_) @function)
(package_declaration name: (_) @namespace)
(package_body name: (_) @namespace)
(generic_instantiation name: (_) @namespace)
(entry_declaration . (identifier) @function)
(procedure_call_statement name: (identifier) @function)
(procedure_call_statement name: (selected_component selector_name: (_) @function))

(full_type_declaration . (identifier) @type)
(private_type_declaration . (identifier) @type)
(incomplete_type_declaration . (identifier) @type)
(private_extension_declaration . (identifier) @type)
(subtype_declaration . (identifier) @type)
(task_type_declaration . (identifier) @type)
(protected_type_declaration . (identifier) @type)
(task_body . (identifier) @type)
(protected_body . (identifier) @type)
(formal_complete_type_declaration . (identifier) @type)
(formal_incomplete_type_declaration . (identifier) @type)
(component_definition subtype_mark: (_) @type)
(parameter_specification subtype_mark: (_) @type)
(result_profile subtype_mark: (_) @type)
(object_declaration subtype_mark: (_) @type)
(discriminant_specification subtype_mark: (_) @type)

(parameter_specification . (identifier) @parameter)
(component_declaration . (identifier) @property)

;; `with` and `use`: what's brought in is a package (a project file's
;; `with "x.gpr"` stays a string).

(use_clause "use" @include "type" @include)
(with_clause "private" @include)
(with_clause "limited" @include)
(use_clause [(identifier) (selected_component)] @namespace)
(with_clause [(identifier) (selected_component)] @namespace)

;; Words that are keywords of another kind where they stand.

(loop_statement "end" @repeat)
(if_statement "end" @conditional)
(case_statement "end" @conditional)
(loop_parameter_specification "in" @repeat)
(iterator_specification ["in" "of"] @repeat)
(raise_statement "with" @keyword.exception)
(raise_expression "with" @keyword.exception)
(full_type_declaration "is" @keyword.type)
(subtype_declaration "is" @keyword.type)
(record_definition "end" @keyword.type)
(array_type_definition "array" @keyword.type "of" @keyword.type)
(range_constraint "range" @keyword.type)
(signed_integer_type_definition "range" @keyword.type)
(record_type_definition ["abstract" "tagged" "limited"] @keyword.type)
(private_type_declaration ["tagged" "limited" "private"] @keyword.type)
(task_type_declaration "task" @keyword.type)

;; Attributes: X'Old, A'Range, T'Image.

(tick) @punctuation.delimiter
(attribute_designator) @property
(range_attribute_designator) @property
(reduction_attribute_designator) @property

;; Contracts: an aspect's name (`Pre`, `Post`, `Global`, `SPARK_Mode`)
;; and a pragma's (`Loop_Invariant`, `Assert`), and the `with` before
;; aspects.

(aspect_specification "with" @keyword)
(aspect_association . (identifier) @attribute)
(pragma_g . (identifier) @attribute)
(global_mode) @keyword

;; GNAT's preprocessor.

(gnatprep_declarative_if_statement) @preproc
(gnatprep_if_statement) @preproc
(gnatprep_identifier) @preproc

;; Labels.

(loop_label (identifier) @label)
(label (identifier) @label)

;; A comment before a declaration documents it.

((comment) @comment.documentation
  . [
      (entry_declaration)
      (subprogram_declaration)
      (parameter_specification)
    ])

(compilation
  . (comment) @comment.documentation)
