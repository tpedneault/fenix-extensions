; Ruby's highlighting, for tree-sitter-ruby 0.23.1.
;
; From that grammar's own queries/highlights.scm (MIT, Copyright (c) 2016
; Rob Rix), with Fenix's capture names: definitions as functions, calls
; as methods, constants as types, `if` and the loops as conditionals and
; repeats, `require` as an include, Ruby's own methods (`puts`, `raise`,
; `attr_reader`) marked, and no `#is-not? local`, which Fenix doesn't
; read. Of two captures of the same text, the later pattern wins.

;;; Names

(identifier) @variable

[
  (instance_variable)
  (class_variable)
] @property

(global_variable) @variable.builtin

[
  (self)
  (super)
] @variable.builtin

(constant) @type

((constant) @constant
  (#match? @constant "^[A-Z][A-Z0-9_]+$"))

; Parameters, a block's and a method's.

(block_parameter (identifier) @parameter)
(block_parameters (identifier) @parameter)
(destructured_parameter (identifier) @parameter)
(hash_splat_parameter (identifier) @parameter)
(lambda_parameters (identifier) @parameter)
(method_parameters (identifier) @parameter)
(splat_parameter (identifier) @parameter)
(keyword_parameter name: (identifier) @parameter)
(optional_parameter name: (identifier) @parameter)

;;; Methods

(method name: [(identifier) (constant) (operator)] @function)
(singleton_method name: [(identifier) (constant) (operator)] @function)
(setter (identifier) @function)
(alias (identifier) @function)

(call method: [(identifier) (constant)] @function.method)

((identifier) @include
  (#any-of? @include "require" "require_relative" "load" "autoload"))

((identifier) @keyword
  (#any-of? @keyword
    "private" "protected" "public" "module_function" "private_constant" "private_class_method"
    "attr_reader" "attr_writer" "attr_accessor" "include" "extend" "prepend" "refine" "using"))

((identifier) @function.builtin
  (#any-of? @function.builtin
    "puts" "print" "p" "pp" "printf" "raise" "fail" "loop" "lambda" "proc" "catch" "throw"
    "block_given?" "binding" "format" "sprintf" "gets" "sleep" "exit" "abort" "at_exit" "freeze"))

"defined?" @function.builtin

;;; Literals

[
  (string)
  (bare_string)
  (subshell)
  (heredoc_body)
  (heredoc_beginning)
  (character)
] @string

[
  (simple_symbol)
  (delimited_symbol)
  (hash_key_symbol)
  (bare_symbol)
] @string.special.symbol

(pair key: (hash_key_symbol) ":" @string.special.symbol)

(regex) @string.regex

(escape_sequence) @escape

[
  (integer)
  (rational)
  (complex)
] @number

(float) @float

[
  (true)
  (false)
] @boolean

(nil) @constant.builtin

[
  (file)
  (line)
  (encoding)
] @constant.builtin

((identifier) @constant.builtin
  (#any-of? @constant.builtin "__FILE__" "__LINE__" "__ENCODING__"))

(interpolation
  "#{" @punctuation.special
  "}" @punctuation.special) @embedded

[
  (comment)
  (uninterpreted)
] @comment

;;; Keywords

[
  "alias"
  "begin"
  "BEGIN"
  "class"
  "do"
  "end"
  "END"
  "ensure"
  "module"
  "rescue"
  "retry"
  "redo"
  "undef"
] @keyword

"def" @keyword.function

[
  "return"
  "yield"
  "break"
  "next"
] @keyword.return

[
  "if"
  "unless"
  "elsif"
  "else"
  "case"
  "when"
  "in"
  "then"
] @conditional

[
  "while"
  "until"
  "for"
] @repeat

; `in` of a `for` is the loop's.
(in "in" @repeat)

[
  "and"
  "or"
  "not"
] @keyword.operator

;;; Operators and punctuation

[
  "="
  "=="
  "!="
  "==="
  "=~"
  "!~"
  "<=>"
  "<"
  ">"
  "<="
  ">="
  "+"
  "-"
  "*"
  "/"
  "%"
  "**"
  "&&"
  "||"
  "!"
  "&"
  "|"
  "^"
  "~"
  "<<"
  ">>"
  "+="
  "-="
  "*="
  "/="
  "||="
  "&&="
  ".."
  "..."
  "=>"
  "->"
  "?"
] @operator

[
  ","
  ";"
  "."
  "&."
  "::"
] @punctuation.delimiter

[
  "("
  ")"
  "["
  "]"
  "{"
  "}"
  "%w("
  "%i("
] @punctuation.bracket

; A block's parameters, between bars.
(block_parameters "|" @punctuation.bracket)
