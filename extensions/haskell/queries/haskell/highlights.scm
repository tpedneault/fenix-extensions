; Haskell's highlighting, for tree-sitter-haskell 0.23.1.
;
; From that grammar's own queries/highlights.scm (MIT, Copyright (c) 2014
; Max Brunsfeld and its contributors): with Fenix's capture names (no
; `@character` or `@spell`, which it doesn't colour; a field is a
; `@property`, a pragma `@preproc`), and its types first rather than
; last: Fenix colours a text by the latest pattern that captures it, so
; the grammar's `(variable) @type` would make every name a type. A type
; variable is a `type/variable` here. Later patterns win where two match
; the same text.

; ----------------------------------------------------------------------------
; Variables, then types: the least specific first.

(variable) @variable

(pattern/wildcard) @variable

(name) @type

(type/star) @type

[
  (type/variable)
  (constraint/variable)
  (type_param/variable)
] @type

(constructor) @constructor

; True or False
((constructor) @boolean
  (#any-of? @boolean "True" "False"))

; ----------------------------------------------------------------------------
; Parameters

(decl/function
  patterns: (patterns
    (_) @variable.parameter))

(expression/lambda
  (_)+ @variable.parameter
  "->")

(decl/function
  (infix
    (pattern) @variable.parameter))

; ----------------------------------------------------------------------------
; Literals and comments

(integer) @number

(negation) @number

(expression/literal
  (float)) @number.float

(char) @string

(string) @string

; unit, as in ()
(unit) @constant.builtin

(comment) @comment

(haddock) @comment.documentation

; ----------------------------------------------------------------------------
; Punctuation

[
  "("
  ")"
  "{"
  "}"
  "["
  "]"
] @punctuation.bracket

[
  ","
  ";"
] @punctuation.delimiter

; ----------------------------------------------------------------------------
; Keywords, operators, imports

"forall" @keyword.repeat

(pragma) @preproc

[
  "if"
  "then"
  "else"
  "case"
  "of"
] @keyword.conditional

[
  "import"
  "qualified"
  "module"
] @keyword.import

[
  (operator)
  (constructor_operator)
  (all_names)
  (wildcard)
  "."
  ".."
  "="
  "|"
  "::"
  "=>"
  "->"
  "<-"
  "\\"
  "`"
  "@"
] @operator

(module
  (module_id) @module)

[
  "where"
  "let"
  "in"
  "class"
  "instance"
  "pattern"
  "data"
  "newtype"
  "family"
  "type"
  "as"
  "hiding"
  "deriving"
  "via"
  "stock"
  "anyclass"
  "do"
  "mdo"
  "rec"
  "infix"
  "infixl"
  "infixr"
] @keyword

; ----------------------------------------------------------------------------
; Functions and variables

(decl
  [
    name: (variable) @function
    names: (binding_list (variable) @function)
  ])

(decl/bind
  name: (variable) @variable)

; Signatures (and the functions they go with) with only one value on the
; right are variables.
(decl/signature
  name: (variable) @variable
  type: (type))

((decl/signature
  name: (variable) @variable.signature
  type: (type))
  .
  (decl
    name: (variable) @variable)
    match: (_)
  (#eq? @variable.signature @variable))

; But one of an IO type is a function.
(decl/signature
  name: (variable) @function
  type: (type/apply
    constructor: (name) @type)
  (#eq? @type "IO"))

((decl/signature
  name: (variable) @function.signature
  type: (type/apply
    constructor: (name) @type)
  (#eq? @type "IO"))
  .
  (decl
    name: (variable) @function)
    match: (_)
  (#eq? @function.signature @function))

((decl/signature) @function
  .
  (decl/function
    name: (variable) @function))

(decl/bind
  name: (variable) @function
  (match
    expression: (expression/lambda)))

; View patterns.
(view_pattern
  [
    (expression/variable) @function.call
    (expression/qualified
      (variable) @function.call)
  ])

; A function in backticks reads as an operator.
(infix_id
  [
    (variable) @operator
    (qualified
      (variable) @operator)
  ])

; A function with an operator after it: func <$> a <*> b.
(infix
  [
    (variable) @function.call
    (qualified
      ((module) @module
        (variable) @function.call))
  ]
  .
  (operator))

; Variables an operator is applied to.
((expression/variable) @variable
  .
  (operator))

((operator)
  .
  [
    (expression/variable) @variable
    (expression/qualified
      (variable) @variable)
  ])

; A function applied with $, <$>, >>= or =<<.
([
    (expression/variable) @function.call
    (expression/qualified
      (variable) @function.call)
  ]
  .
  (operator) @operator
  (#any-of? @operator "$" "<$>" ">>=" "=<<"))

((infix
  [
    (operator)
    (infix_id (variable))
  ]
  .
  [
    (variable) @function.call
    (qualified
      (variable) @function.call)
  ])
  .
  (operator) @operator
  (#any-of? @operator "$" "<$>" "=<<"))

; Composition, arrows, monadic composition: on the left,
(
  [
    (expression/variable) @function
    (expression/qualified
      (variable) @function)
  ]
  .
  (operator) @operator
  (#any-of? @operator "." ">>>" "***" ">=>" "<=<"))

((infix
  [
    (operator)
    (infix_id (variable))
  ]
  .
  [
    (variable) @function
    (qualified
      (variable) @function)
  ])
  .
  (operator) @operator
  (#any-of? @operator "." ">>>" "***" ">=>" "<=<"))

; and on the right.
((operator) @operator
  .
  [
    (expression/variable) @function
    (expression/qualified
      (variable) @function)
  ]
  (#any-of? @operator "." ">>>" "***" ">=>" "<=<"))

; A function defined as a composition.
(decl/function
  name: (variable) @function
  (match
    expression: (infix
      operator: (operator) @operator
      (#any-of? @operator "." ">>>" "***" ">=>" "<=<"))))

(apply
  [
    (expression/variable) @function.call
    (expression/qualified
      (variable) @function.call)
  ])

; A composition in parentheses, applied: on the left,
(apply
  .
  (expression/parens
    (infix
      [
        (variable) @function.call
        (qualified
          (variable) @function.call)
      ]
      .
      (operator))))

; and on the right.
(apply
  .
  (expression/parens
    (infix
      (operator)
      .
      [
        (variable) @function.call
        (qualified
          (variable) @function.call)
      ])))

; Variables given to a function.
(apply
  (_)
  .
  [
    (expression/variable) @variable
    (expression/qualified
      (variable) @variable)
  ])

; main is always a function: `main = undefined` too.
(decl/bind
  name: (variable) @function
  (#eq? @function "main"))

; Scoped function types (func :: a -> b).
(signature
  pattern: (pattern/variable) @function
  type: (quantified_type))

; Signatures of a function type, and the bindings after them.
(decl/signature
  name: (variable) @function
  type: (quantified_type))

((decl/signature
  name: (variable) @function.signature
  type: (quantified_type))
  .
  (decl/bind
    (variable) @function)
  (#eq? @function @function.signature))

; otherwise (= True)
((variable) @boolean
  (#eq? @boolean "otherwise"))

; ----------------------------------------------------------------------------
; Quasi-quotes

(quoter) @function.call

(quasiquote
  [
    (quoter) @function.call
    (_
      (variable) @function.call)
  ]
  (#eq? @function.call "qq")
  (quasiquote_body) @string)

; A quoter of a module's.
(quasiquote
  (_
    (module) @module
    .
    (variable) @function.call))

; ----------------------------------------------------------------------------
; Exceptions and error handling

((variable) @keyword.exception
  (#any-of? @keyword.exception
    "error" "undefined" "try" "tryJust" "tryAny" "catch" "catches" "catchJust" "handle" "handleJust"
    "throw" "throwIO" "throwTo" "throwError" "ioError" "mask" "mask_" "uninterruptibleMask"
    "uninterruptibleMask_" "bracket" "bracket_" "bracketOnErrorSource" "finally" "fail"
    "onException" "expectationFailure"))

; ----------------------------------------------------------------------------
; Debugging

((variable) @keyword.debug
  (#any-of? @keyword.debug
    "trace" "traceId" "traceShow" "traceShowId" "traceWith" "traceShowWith" "traceStack" "traceIO"
    "traceM" "traceShowM" "traceEvent" "traceEventWith" "traceEventIO" "flushEventLog" "traceMarker"
    "traceMarkerIO"))

; ----------------------------------------------------------------------------
; Fields, and the names an import lists

(field_name
  (variable) @property)

(import_name
  (name)
  .
  (children
    (variable) @property))

(import_name
  variable: (variable) @function)

(export
  variable: (variable) @function)
