; .http files: requests, their headers and variables. Captures are the
; names Fenix's own grammars use, so the theme colours them.

(comment) @comment
(request_separator) @comment

; # @name login, # @no-redirect
(comment
  "@" @keyword
  name: (_) @keyword)

(method) @keyword
(request
  url: (_) @string)
(http_version) @constant

(header
  name: (_) @property)
(header
  ":" @punctuation.delimiter)

(variable_declaration
  "@" @keyword
  name: (identifier) @variable)
(variable_declaration
  "=" @operator)

(variable) @variable.builtin
[
  "{{"
  "}}"
] @punctuation.bracket

(status_code) @number
(status_text) @string

(external_body
  path: (_) @string.special)
(res_redirect
  path: (_) @string.special)
