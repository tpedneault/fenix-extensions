; Highlights for Scala 2 and 3, for tree-sitter-scala 0.24.0's grammar.
; Adapted from the queries that grammar's package ships
; (queries/highlights.scm, MIT, credited there to @stumash): written again
; for Fenix's capture names -- no `function.call`, `method.call` or `exception`,
; which it has no colours for, and a modifier is a keyword -- and with the
; Scala 3 forms (`given`, `using`, `extension`, `enum`, indentation syntax,
; `end` markers, `derives`) and the pack's own.
; Later patterns win.

; Names: a bare name is a variable, a capitalised one a type or an object,
; ALL_CAPS a constant; `.name` is a member.
(identifier) @variable
((identifier) @type
  (#match? @type "^[A-Z]"))
((identifier) @constant
  (#match? @constant "^[A-Z][A-Z0-9_]+$"))
(field_expression field: (identifier) @property)
((field_expression field: (identifier) @type)
  (#match? @type "^[A-Z]"))
(type_identifier) @type
(wildcard) @variable.builtin
((identifier) @variable.builtin
  (#match? @variable.builtin "^(this|super)$"))
(operator_identifier) @operator

; Imports, exports and packages: the path is namespaces, a capitalised
; name in it a type.
(package_identifier (identifier) @namespace)
(import_declaration path: (identifier) @namespace)
(export_declaration path: (identifier) @namespace)
(stable_identifier (identifier) @namespace)
((import_declaration path: (identifier) @type)
  (#match? @type "^[A-Z]"))
((export_declaration path: (identifier) @type)
  (#match? @type "^[A-Z]"))
((stable_identifier (identifier) @type)
  (#match? @type "^[A-Z]"))
(namespace_selectors (identifier) @variable)
((namespace_selectors (identifier) @type)
  (#match? @type "^[A-Z]"))
(namespace_wildcard) @operator
(as_renamed_identifier alias: (identifier) @type)
(arrow_renamed_identifier alias: (identifier) @type)

; Definitions: what a `class`, `trait`, `object`, `enum`, `type`, `def`,
; `val`, `var` or `given` names.
(class_definition name: (identifier) @type)
(trait_definition name: (identifier) @type)
(object_definition name: (identifier) @type)
(enum_definition name: (identifier) @type)
(simple_enum_case name: (identifier) @constant)
(full_enum_case name: (identifier) @type)
(package_object name: (identifier) @type)
(type_definition name: (type_identifier) @type)
(given_definition name: (identifier) @function)
(function_definition name: (identifier) @function)
(function_declaration name: (identifier) @function)
(val_definition pattern: (identifier) @variable)
(var_definition pattern: (identifier) @variable)
(val_declaration name: (identifier) @variable)
(var_declaration name: (identifier) @variable)
(parameter name: (identifier) @parameter)
(class_parameter name: (identifier) @parameter)
(binding name: (identifier) @parameter)
(self_type (identifier) @parameter)
(capture_pattern name: (identifier) @variable)

; Calls: `f(x)`, `x.f(y)`, `f[T](x)`, `Point(1, 2)` (a capitalised name is a
; type), `s"..."`'s `s`.
(call_expression function: (identifier) @function)
(call_expression function: (operator_identifier) @function)
(call_expression function: (field_expression field: (identifier) @function))
(generic_function function: (identifier) @function)
(generic_function function: (field_expression field: (identifier) @function))
((call_expression function: (identifier) @type)
  (#match? @type "^[A-Z]"))
(interpolated_string_expression interpolator: (identifier) @function)

; Operators: `a + b`, `a map f`, `x :: xs`, `A | B`.
(infix_expression operator: (identifier) @operator)
(infix_expression operator: (operator_identifier) @operator)
(infix_type operator: (operator_identifier) @operator)
(infix_pattern operator: (identifier) @operator)
(infix_pattern operator: (operator_identifier) @operator)
["=>" "?=>" "<-" "=" "<:" ">:" "<%" "=>>" "|"] @operator
(infix_modifier) @keyword

; Literals.
(boolean_literal) @boolean
(integer_literal) @number
(floating_point_literal) @number
(null_literal) @constant.builtin
[
  (string)
  (character_literal)
  (quote_expression)
  (interpolated_string)
  (interpolated_string_expression)
] @string
(escape_sequence) @string.escape
(interpolation "$" @punctuation.special)
(interpolation (identifier) @variable)
(interpolation (block "{" @punctuation.special))
(interpolation (block "}" @punctuation.special))

; Keywords: what declares, what modifies, what controls, what a Scala 3
; `given` or `extension` reads as.
[
  "case" "class" "enum" "extends" "derives" "object" "package" "trait"
  "type" "val" "var" "with" "given" "using" "end" "extension" "macro"
  "import" "export" "as" "def" "new" "return" "throw"
] @keyword
[
  "abstract" "final" "lazy" "sealed" "private" "protected" "inline"
  "transparent" "opaque" "open" "infix" "override" "implicit"
] @keyword
(opaque_modifier) @keyword
(transparent_modifier) @keyword
(open_modifier) @keyword
(inline_modifier) @keyword
(access_modifier) @keyword
(modifiers) @keyword
[ "if" "else" "match" "then" ] @conditional
[ "do" "for" "while" "yield" ] @repeat
[ "try" "catch" "finally" ] @keyword
(case_block (case_clause "case" @conditional))
(indented_cases (case_clause "case" @conditional))

; Annotations: `@main`, `@tailrec`, `@deprecated("x", "1.0")`: the `@` and
; the name.
(annotation "@" @attribute)
(annotation name: (_) @attribute)

; Punctuation.
["(" ")" "[" "]" "{" "}"] @punctuation.bracket
["." "," ";"] @punctuation.delimiter

; Comments, and a Scala CLI `//> using` directive: its key and value.
(comment) @comment
(block_comment) @comment
(using_directive_key) @attribute
(using_directive_value) @string
