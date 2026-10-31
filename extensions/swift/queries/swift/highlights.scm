; Swift's highlighting, for tree-sitter-swift 0.7.1 (alex-pinkus).
;
; From that grammar's own queries/highlights.scm (MIT, Copyright (c) 2021
; alex-pinkus), with Fenix's capture names (no `@spell`,
; `@character.special` or `@keyword.directive`, which it doesn't colour)
; and `import`, `case`, `in`, `return` and the like as keywords. Later
; patterns win where two match the same text.

[
  "."
  ";"
  ":"
  ","
] @punctuation.delimiter

[
  "("
  ")"
  "["
  "]"
  "{"
  "}"
] @punctuation.bracket

;;; Identifiers

(simple_identifier) @variable

(type_identifier) @type

[
  (self_expression)
  (super_expression)
] @variable.builtin

;;; Declarations

[
  "func"
  "deinit"
] @keyword.function

[
  (visibility_modifier)
  (member_modifier)
  (function_modifier)
  (property_modifier)
  (parameter_modifier)
  (inheritance_modifier)
  (mutation_modifier)
] @keyword.modifier

(function_declaration
  (simple_identifier) @function.method)

(protocol_function_declaration
  name: (simple_identifier) @function.method)

(init_declaration
  "init" @constructor)

(parameter
  external_name: (simple_identifier) @parameter)

(parameter
  name: (simple_identifier) @parameter)

(type_parameter
  (type_identifier) @type)

[
  "protocol"
  "extension"
  "indirect"
  "nonisolated"
  "override"
  "convenience"
  "required"
  "some"
  "any"
  "weak"
  "unowned"
  "didSet"
  "willSet"
  "subscript"
  "let"
  "var"
  (throws)
  (where_keyword)
  (getter_specifier)
  (setter_specifier)
  (modify_specifier)
  (else)
  (as_operator)
] @keyword

[
  "enum"
  "struct"
  "class"
  "actor"
  "typealias"
] @keyword.type

[
  "async"
  "await"
] @keyword.coroutine

(shebang_line) @preproc

(class_body
  (property_declaration
    (pattern
      (simple_identifier) @property)))

(protocol_property_declaration
  (pattern
    (simple_identifier) @property))

(navigation_expression
  (navigation_suffix
    (simple_identifier) @property))

(value_argument
  name: (value_argument_label
    (simple_identifier) @property))

(import_declaration
  "import" @keyword)

(import_declaration
  (identifier
    (simple_identifier) @namespace))

(enum_entry
  "case" @keyword)

(enum_entry
  (simple_identifier) @constant)

(attribute
  "@" @attribute
  (user_type
    (type_identifier) @attribute))

;;; Calls

(call_expression
  (simple_identifier) @function.call)

; foo.bar.baz(): baz
(call_expression
  (navigation_expression
    (navigation_suffix
      (simple_identifier) @function.call)))

; .foo()
(call_expression
  (prefix_expression
    (simple_identifier) @function.call))

; SomeType.method(): SomeType as a type
((navigation_expression
  (simple_identifier) @type)
  (#match? @type "^[A-Z]"))

; A call of a type's name: a constructor.
((call_expression
  (simple_identifier) @constructor)
  (#match? @constructor "^[A-Z]"))

((call_expression
  (simple_identifier) @function.builtin)
  (#any-of? @function.builtin "print" "debugPrint" "dump" "fatalError" "precondition" "preconditionFailure" "assert" "assertionFailure" "min" "max" "abs" "zip" "stride" "type" "withUnsafePointer"))

(directive) @preproc

[
  (diagnostic)
  (availability_condition)
  (playground_literal)
  (key_path_string_expression)
  (selector_expression)
  (external_macro_definition)
] @function.macro

(special_literal) @constant.macro

;;; Statements

(for_statement
  "for" @repeat)

(for_statement
  "in" @repeat)

[
  "while"
  "repeat"
  "continue"
  "break"
] @repeat

(guard_statement
  "guard" @conditional)

(if_statement
  "if" @conditional)

(switch_statement
  "switch" @conditional)

(switch_entry
  "case" @keyword)

(switch_entry
  "fallthrough" @keyword)

(switch_entry
  (default_keyword) @keyword)

"return" @keyword.return

(ternary_expression
  [
    "?"
    ":"
  ] @conditional)

[
  (try_operator)
  "do"
  (throw_keyword)
  (catch_keyword)
] @keyword.exception

(statement_label) @label

;;; Comments

[
  (comment)
  (multiline_comment)
] @comment

((comment) @comment.documentation
  (#match? @comment.documentation "^///"))

((multiline_comment) @comment.documentation
  (#match? @comment.documentation "^/[*][*][^*]"))

;;; Strings

(line_str_text) @string

(str_escaped_char) @escape

(multi_line_str_text) @string

(raw_str_part) @string

(raw_str_end_part) @string

(line_string_literal
  [
    "\\("
    ")"
  ] @punctuation.special)

(multi_line_string_literal
  [
    "\\("
    ")"
  ] @punctuation.special)

(raw_str_interpolation
  [
    (raw_str_interpolation_start)
    ")"
  ] @punctuation.special)

[
  "\""
  "\"\"\""
] @string

(regex_literal) @string.regexp

;;; Closures

(lambda_literal
  "in" @keyword.operator)

;;; Literals

[
  (integer_literal)
  (hex_literal)
  (oct_literal)
  (bin_literal)
] @number

(real_literal) @float

(boolean_literal) @boolean

"nil" @constant.builtin

(wildcard_pattern) @variable.builtin

;;; Operators

(custom_operator) @operator

[
  "+"
  "-"
  "*"
  "/"
  "%"
  "="
  "+="
  "-="
  "*="
  "/="
  "<"
  ">"
  "<<"
  ">>"
  "<="
  ">="
  "++"
  "--"
  "^"
  "&"
  "&&"
  "|"
  "||"
  "~"
  "%="
  "!="
  "!=="
  "=="
  "==="
  "?"
  "??"
  "->"
  "..<"
  "..."
  (bang)
] @operator

(type_arguments
  [
    "<"
    ">"
  ] @punctuation.bracket)
