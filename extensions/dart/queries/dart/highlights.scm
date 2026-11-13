; Dart's highlighting: tree-sitter-dart's own queries (UserNobody14's, MIT,
; at the commit grammars/README.md names), with Fenix's capture names, and
; the pack's own for what Dart 3 added.

(identifier) @variable

; Keywords
; --------------------
[
  (assert_builtin)
  (const_builtin)
  (final_builtin)
  (part_of_builtin)
  (case_builtin)
  (void_type)
  "abstract"
  "as"
  "base"
  "class"
  "covariant"
  "default"
  "deferred"
  "enum"
  "export"
  "extends"
  "extension"
  "external"
  "factory"
  "Function"
  "hide"
  "implements"
  "import"
  "in"
  "interface"
  "is"
  "late"
  "library"
  "mixin"
  "new"
  "on"
  "part"
  "required"
  "sealed"
  "show"
  "static"
  "typedef"
  "var"
  "with"
] @keyword

["if" "else" "switch" "case" "when"] @conditional
["for" "while" "do"] @repeat
[(break_builtin) "continue"] @repeat
"return" @keyword.return
[(rethrow_builtin) "try" "catch" "finally" "throw"] @keyword.exception
["async" "async*" "sync*" "await" "yield"] @keyword.coroutine
["super"] @variable.builtin

; Functions and methods
; --------------------

; The grammar has no call node: a name followed by arguments is one.
(((identifier) @function
  (#match? @function "^_?[a-z]"))
 .
 (selector . (argument_part))) @function.call

(function_signature name: (identifier) @function)
(getter_signature "get" @keyword (identifier) @function)
(setter_signature "set" @keyword name: (identifier) @function)
(operator_signature "operator" @keyword)

((selector
  (unconditional_assignable_selector (identifier) @function.method))
 (selector (argument_part (arguments))))

(cascade_section
  (cascade_selector (identifier) @function.method)
  (argument_part (arguments)))

; Types
; --------------------
(type_identifier) @type
((type_identifier) @type.builtin
  (#match? @type.builtin "^(int|double|num|String|bool|List|Set|Map|Runes|Symbol|Object|dynamic|Never|Null|Future|Stream|Iterable|Record|Type)$"))
(class_definition name: (identifier) @type)
(enum_declaration name: (identifier) @type)
(extension_type_declaration name: (identifier) @type)
(mixin_declaration (identifier) @type)
(type_alias (type_identifier) @type)
(constructor_signature name: (identifier) @constructor)
(factory_constructor_signature (identifier) @constructor)
(primary_constructor name: (identifier) @constructor)
(scoped_identifier scope: (identifier) @type)
((identifier) @type
  (#match? @type "^_?[A-Z].*[a-z]"))
("Function" @type)
(inferred_type) @keyword
(extension_type_declaration "type" @keyword)

(enum_constant name: (identifier) @constant)
(this) @variable.builtin

; Properties and parameters
; --------------------
(unconditional_assignable_selector (identifier) @property)
(conditional_assignable_selector (identifier) @property)
(cascade_section (cascade_selector (identifier) @property))
(representation_declaration name: (identifier) @property)
(object_pattern (identifier) @property)
(formal_parameter name: (identifier) @parameter)
(super_formal_parameter (identifier) @parameter)
(constructor_param (identifier) @parameter)
(named_argument (label (identifier) @parameter))
(label (identifier) @label)

; Literals
; --------------------
[(hex_integer_literal) (decimal_integer_literal)] @number
(decimal_floating_point_literal) @float
(string_literal) @string
(escape_sequence) @escape
(symbol_literal) @constant
[(true) (false)] @boolean
(null_literal) @constant.builtin

(template_substitution
  "$" @punctuation.special
  "{" @punctuation.special
  "}" @punctuation.special)
(template_substitution
  "$" @punctuation.special
  (identifier_dollar_escaped) @variable)

; Comments and annotations
; --------------------
(documentation_comment) @comment.documentation
(comment) @comment
(script_tag) @comment
(annotation "@" @attribute name: (identifier) @attribute)

; Operators and punctuation
; --------------------
[
  "=>" ".." "?." "??" "??=" "==" "?" ":" "&&" "%" "<" ">" "=" ">=" "<=" "||" "~/" "..."
  (increment_operator)
  (is_operator)
  (as_operator)
  (prefix_operator)
  (equality_operator)
  (additive_operator)
  (multiplicative_operator)
  (relational_operator)
  (shift_operator)
  (bitwise_operator)
] @operator

(type_arguments ["<" ">"] @punctuation.bracket)
(type_parameters ["<" ">"] @punctuation.bracket)
["(" ")" "[" "]" "{" "}"] @punctuation.bracket
[";" "." ","] @punctuation.delimiter
