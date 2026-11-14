; What colours Elixir, for tree-sitter-elixir 0.3.5. From the grammar's own
; highlights.scm (Apache-2.0), with Fenix's capture names, the captures
; its predicates used as placeholders given names of their own, and atoms
; and keywords read as constants.

; Punctuation

"%" @punctuation

[
  ","
  ";"
] @punctuation.delimiter

[
  "("
  ")"
  "["
  "]"
  "{"
  "}"
  "<<"
  ">>"
] @punctuation.bracket

; Literals

[
  (boolean)
  (nil)
] @constant

[
  (integer)
  (float)
] @number

(char) @constant

; Identifiers

(identifier) @variable

; * unused: a name starting with an underscore
((identifier) @comment.unused
  (#match? @comment.unused "^_"))

; * special forms
((identifier) @constant.builtin
  (#any-of? @constant.builtin "__MODULE__" "__DIR__" "__ENV__" "__CALLER__" "__STACKTRACE__"))

; Comments

(comment) @comment

; Quoted content

(interpolation
  "#{" @punctuation.special
  "}" @punctuation.special)

(escape_sequence) @string.escape

[
  (string)
  (charlist)
] @string

[
  (atom)
  (quoted_atom)
] @constant

[
  (keyword)
  (quoted_keyword)
] @property

; * sigils: the whole of one a string, its name and its delimiters set apart
(sigil) @string

(sigil
  (sigil_name) @string.special
  quoted_start: _ @string.special
  quoted_end: _ @string.special)

((sigil
  (sigil_name) @string.regex.name) @string.regex
  (#match? @string.regex.name "^[rR]$"))

; Calls

; * local function call
(call
  target: (identifier) @function)

; * remote function call
(call
  target: (dot
    right: (identifier) @function))

; * field without parentheses or block
(call
  target: (dot
    right: (identifier) @property)
  .)

; * remote call without parentheses or block (overrides the above)
(call
  target: (dot
    left: [
      (alias)
      (atom)
    ]
    right: (identifier) @function)
  .)

; * definitions
((call
  target: (identifier) @keyword)
  (#any-of? @keyword "def" "defdelegate" "defexception" "defguard" "defguardp" "defimpl" "defmacro" "defmacrop" "defmodule" "defn" "defnp" "defoverridable" "defp" "defprotocol" "defstruct"))

; * Kernel's and the special forms'
((call
  target: (identifier) @keyword)
  (#any-of? @keyword "alias" "case" "cond" "for" "if" "import" "quote" "raise" "receive" "require" "reraise" "super" "throw" "try" "unless" "unquote" "unquote_splicing" "use" "with"))

; * the name in a function definition
((call
  target: (identifier) @keyword.definition
  (arguments
    [
      (identifier) @function
      (binary_operator
        left: (identifier) @function
        operator: "when")
    ]))
  (#any-of? @keyword.definition "def" "defdelegate" "defguard" "defguardp" "defmacro" "defmacrop" "defn" "defnp" "defp"))

; * a pipe into a function
(binary_operator
  operator: "|>"
  right: (identifier) @function)

((call
  target: (identifier) @keyword.definition
  (arguments
    (binary_operator
      operator: "|>"
      right: (identifier) @variable)))
  (#any-of? @keyword.definition "def" "defdelegate" "defguard" "defguardp" "defmacro" "defmacrop" "defn" "defnp" "defp"))

(binary_operator
  operator: "|>"
  right: (call
    target: (dot
      right: (identifier) @function)))

; Operators

(unary_operator
  operator: "&"
  operand: (integer) @operator)

(operator_identifier) @operator

(unary_operator
  operator: _ @operator)

(binary_operator
  operator: _ @operator)

(dot
  operator: _ @operator)

(stab_clause
  operator: _ @operator)

; Module attributes: @name and @name value

(unary_operator
  operator: "@" @attribute
  operand: [
    (identifier) @attribute
    (call
      target: (identifier) @attribute)
    (boolean) @attribute
    (nil) @attribute
  ])

; * documentation: @doc, @moduledoc and @typedoc with a string or a sigil
(unary_operator
  operator: "@" @comment.documentation
  operand: (call
    target: (identifier) @comment.documentation.name
    (arguments
      [
        (string) @comment.documentation
        (charlist) @comment.documentation
        (sigil) @comment.documentation
        (boolean) @comment.documentation
      ]))
  (#any-of? @comment.documentation.name "moduledoc" "typedoc" "doc"))

; Modules

(alias) @module

(call
  target: (dot
    left: (atom) @module))

; Reserved words

[
  "when"
  "and"
  "or"
  "not"
  "in"
  "not in"
  "fn"
  "do"
  "end"
  "catch"
  "rescue"
  "after"
  "else"
] @keyword
