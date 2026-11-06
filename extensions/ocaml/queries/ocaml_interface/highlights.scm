; OCaml's interfaces' highlighting (.mli), for tree-sitter-ocaml 0.24.2's
; interface grammar: queries/ocaml/highlights.scm, the implementations',
; without its `(shebang)`, which an interface has none of.
;
; From that grammar's own queries/highlights.scm (MIT, Copyright (c) 2020
; Max Brunsfeld and Pieter Goetschalckx): with Fenix's capture names (an
; attribute's name is an `@attribute`, not a `@tag`, which Fenix colours
; as a keyword; `true` and `false` are `@boolean`), its keywords told
; apart (conditionals, loops, `open`, `fun`, `try`, modifiers), floats
; and doc comments (`(** ... *)`) told from the rest by `#match?`, which
; tree-sitter runs. Later patterns win where two match the same text.

; Punctuation
;------------

[
  "," "." ";" ":" "=" "|" "~" "?" "+" "-" "!" ">" "&"
  "->" ";;" ":>" "+=" ":=" ".."
] @punctuation.delimiter

["(" ")" "[" "]" "{" "}" "[|" "|]" "[<" "[>"] @punctuation.bracket

(object_type ["<" ">"] @punctuation.bracket)

"%" @punctuation.special

(attribute ["[@" "]"] @punctuation.special)
(item_attribute ["[@@" "]"] @punctuation.special)
(floating_attribute ["[@@@" "]"] @punctuation.special)
(extension ["[%" "]"] @punctuation.special)
(item_extension ["[%%" "]"] @punctuation.special)
(quoted_extension ["{%" "}"] @punctuation.special)
(quoted_item_extension ["{%%" "}"] @punctuation.special)

; Keywords
;---------

[
  "and" "as" "assert" "begin" "class" "constraint" "effect" "end"
  "external" "functor" "in" "inherit" "initializer" "lazy" "let" "method"
  "module" "new" "nonrec" "object" "of" "rec" "sig" "struct" "type" "val"
  "with"
] @keyword

["if" "then" "else" "match" "when"] @keyword.conditional

["for" "while" "do" "done" "to" "downto"] @keyword.repeat

["open" "include"] @keyword.import

["fun" "function"] @keyword.function

["try" "exception"] @keyword.exception

["mutable" "private" "virtual"] @keyword.modifier

; Operators
;----------

[
  (prefix_operator)
  (sign_operator)
  (pow_operator)
  (mult_operator)
  (add_operator)
  (concat_operator)
  (rel_operator)
  (and_operator)
  (or_operator)
  (assign_operator)
  (hash_operator)
  (indexing_operator)
  (let_operator)
  (let_and_operator)
  (match_operator)
] @operator

(match_expression (match_operator) @keyword)

(value_definition [(let_operator) (let_and_operator)] @keyword)

["*" "#" "::" "<-"] @operator

; Constants
;----------

(boolean) @boolean

[(number) (signed_number)] @number

((number) @number.float
  (#match? @number.float "^[0-9][0-9_]*([.]|[eE])"))

[(string) (character)] @string

(quoted_string "{" @string "}" @string) @string

(escape_sequence) @string.escape

(conversion_specification) @string.special

; Variables
;----------

[(value_name) (type_variable)] @variable

(value_pattern) @variable.parameter

; Properties
;-----------

[(label_name) (field_name) (instance_variable_name)] @property

; Functions
;----------

(let_binding
  pattern: (value_name) @function
  (parameter))

(let_binding
  pattern: (value_name) @function
  body: [(fun_expression) (function_expression)])

(value_specification (value_name) @function)

(external (value_name) @function)

(method_name) @function.method

(application_expression
  function: (value_path (value_name) @function.call))

(infix_expression
  left: (value_path (value_name) @function.call)
  operator: (concat_operator) @operator
  (#eq? @operator "@@"))

(infix_expression
  operator: (rel_operator) @operator
  right: (value_path (value_name) @function.call)
  (#eq? @operator "|>"))

((value_name) @function.builtin
  (#match? @function.builtin "^(raise(_notrace)?|failwith|invalid_arg)$"))

; Types
;------

[(class_name) (class_type_name) (type_constructor)] @type

((type_constructor) @type.builtin
  (#match? @type.builtin "^(int|char|bytes|string|float|bool|unit|exn|array|list|option|int32|int64|nativeint|format6|lazy_t)$"))

[(constructor_name) (tag)] @constructor

; Modules
;--------

[(module_name) (module_type_name)] @module

; Attributes
;-----------

(attribute_id) @attribute

; Comments
;---------

[(comment) (line_number_directive) (directive)] @comment

((comment) @comment.documentation
  (#match? @comment.documentation "^[(][*][*]([^*]|$)"))
