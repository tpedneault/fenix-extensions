; Highlights for Nix, for tree-sitter-nix 0.3.0's grammar (revision
; ea1d87f), after the grammar's own queries/highlights.scm (MIT), with
; Fenix's capture names. Later patterns win.

(comment) @comment

; Names: a bare name is a variable; `pkgs.hello`'s `hello` and a binding's
; name are attributes, what a set holds.
(identifier) @variable
(variable_expression name: (identifier) @variable)

(binding attrpath: (attrpath attr: (identifier) @property))
(select_expression attrpath: (attrpath attr: (identifier) @property))
(has_attr_expression attrpath: (attrpath attr: (identifier) @property))
(inherited_attrs attr: (identifier) @property)

; A function's arguments: `{ pkgs, lib ? 1, ... }:` and `x: ...`, `args@{ ... }`.
(function_expression universal: (identifier) @variable.parameter)
(formal name: (identifier) @variable.parameter)
(ellipses) @punctuation.special

; Calls: `f x`, and the last attribute of `lib.mkIf x`.
(apply_expression function: (variable_expression name: (identifier) @function))
(apply_expression function: (select_expression attrpath: (attrpath attr: (identifier) @function .)))

; What the language gives every expression.
((variable_expression name: (identifier) @variable.builtin)
  (#match? @variable.builtin "^(builtins|__currentSystem|__currentTime|__langVersion|__nixPath|__nixVersion|__storeDir)$"))
((variable_expression name: (identifier) @function.builtin)
  (#match? @function.builtin "^(abort|baseNameOf|break|derivation|derivationStrict|dirOf|fetchGit|fetchMercurial|fetchTarball|fetchTree|fromTOML|import|isNull|map|placeholder|removeAttrs|scopedImport|throw|toString)$"))
((variable_expression name: (identifier) @boolean)
  (#match? @boolean "^(true|false)$"))
((variable_expression name: (identifier) @constant.builtin)
  (#eq? @constant.builtin "null"))

; Literals.
(integer_expression) @number
(float_expression) @number
(string_expression "\"" @string)
(string_fragment) @string
(indented_string_expression "''" @string)
(escape_sequence) @string.escape
(dollar_escape) @string.escape
[
  (path_expression)
  (hpath_expression)
  (spath_expression)
] @string.special
(uri_expression) @string.special

; `${ ... }` is code in a string.
(interpolation "${" @punctuation.special)
(interpolation "}" @punctuation.special)

; Keywords.
[ "if" "then" "else" ] @keyword.conditional
[ "let" "in" "inherit" "rec" "with" "assert" "or" ] @keyword

; Operators and punctuation.
(unary_expression operator: _ @operator)
(binary_expression operator: _ @operator)
[ "?" "@" ":" ] @operator
[ ";" "." "," "=" ] @punctuation.delimiter
[ "(" ")" "[" "]" "{" "}" ] @punctuation.bracket
