; Typst, for tree-sitter-typst at 46cf4de (the grammar's own queries, in
; Fenix's capture names, and the pack's additions). A later pattern wins.

(ident) @variable
(comment) @comment

; Markup
(heading "=" @markup.heading) @text.title
(heading "==" @markup.heading)
(heading "===" @markup.heading)
(heading "====" @markup.heading)
(heading "=====" @markup.heading)
(heading "======" @markup.heading)
(strong) @text.strong
(emph) @text.emphasis
(url) @text.uri
(label) @label
(ref) @text.reference
(item "-" @punctuation.special)
(term ["/" ":"] @punctuation.special)
(quote) @string
(symbol) @constant
(shorthand) @constant
(linebreak) @constant
(escape) @string.escape
(raw_blck "```" @punctuation.special) @text.literal
(raw_blck lang: (ident) @label)
(raw_span "`" @punctuation.special) @text.literal

; Code
(let "let" @keyword)
(branch ["if" "else"] @keyword)
(while "while" @keyword)
(for ["for" "in"] @keyword)
(import "import" @keyword)
(include "include" @keyword)
(show "show" @keyword)
(set "set" @keyword)
(return "return" @keyword)
(flow ["break" "continue"] @keyword)
(context "context" @keyword)
(as "as" @keyword)
(in ["in" "not"] @keyword)
(and "and" @keyword)
(or "or" @keyword)
(not "not" @keyword)
(sign ["+" "-"] @operator)
(add "+" @operator)
(sub "-" @operator)
(mul "*" @operator)
(div "/" @operator)
(cmp ["==" "<=" ">=" "!=" "<" ">"] @operator)
(wildcard) @operator
(assign "assign" @operator)
(number) @number
(string) @string
(bool) @boolean
(none) @constant
(auto) @constant

; Calls, fields and parameters
(call item: (ident) @function)
(call item: (field field: (ident) @function))
(field field: (ident) @property)
(tagged field: (ident) @property)
(lambda pattern: (ident) @parameter)

; Math
(math "$" @punctuation.special)
(attach ["^" "_"] @operator)
(fraction "/" @operator)
(fac "!" @operator)
(letter) @variable

"#" @punctuation.special
["(" ")" "{" "}"] @punctuation.bracket
(content ["[" "]"] @punctuation.bracket)
["," ";" ".." ":"] @punctuation.delimiter
