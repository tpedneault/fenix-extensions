; Highlights for Terraform and HCL, for tree-sitter-hcl 1.2.0's grammars
; (the `terraform` dialect and `hcl`, which share their node names). Written
; for Fenix's capture names; the grammar's repository ships no queries of
; its own. Later patterns win.

; Names: a bare name is a variable; a block's type and its labels, an
; attribute's name, an object's keys and `.name` are what they are.
(identifier) @variable
(variable_expr (identifier) @variable)

(attribute (identifier) @property)
(object_elem key: (expression (variable_expr (identifier) @property)))
(get_attr (identifier) @property)
(legacy_index) @number

; `resource`, `variable`, `locals`, `dynamic`, `lifecycle`: the word that
; says what a block is.
(block . (identifier) @keyword)

; What Terraform's expressions start from.
((variable_expr (identifier) @variable.builtin)
  (#match? @variable.builtin "^(var|local|module|data|each|count|path|terraform|self)$"))

; Functions, a provider's too: provider::aws::arn_parse.
(function_call (identifier) @function)
((function_call (identifier) @function.builtin)
  (#match? @function.builtin "^(abs|abspath|alltrue|anytrue|base64decode|base64encode|basename|bcrypt|can|ceil|chomp|chunklist|cidrhost|cidrnetmask|cidrsubnet|cidrsubnets|coalesce|coalescelist|compact|concat|contains|csvdecode|dirname|distinct|element|endswith|file|filebase64|fileexists|fileset|flatten|floor|format|formatdate|formatlist|indent|index|join|jsondecode|jsonencode|keys|length|log|lookup|lower|matchkeys|max|md5|merge|min|nonsensitive|one|parseint|pathexpand|pow|range|regex|regexall|replace|reverse|rsadecrypt|sensitive|setintersection|setproduct|setsubtract|setunion|sha1|sha256|sha512|signum|slice|sort|split|startswith|strcontains|strrev|substr|sum|templatefile|textdecodebase64|textencodebase64|timeadd|timecmp|timestamp|title|tobool|tolist|tomap|tonumber|toset|tostring|transpose|trim|trimprefix|trimspace|trimsuffix|try|type|upper|urlencode|uuid|uuidv5|values|yamldecode|yamlencode|zipmap)$"))

; Literals.
(bool_lit) @boolean
(null_lit) @constant.builtin
(numeric_lit) @number
(string_lit) @string
(quoted_template_start) @string
(quoted_template_end) @string
(template_literal) @string
(heredoc_start) @string
(heredoc_identifier) @string.special

; A block's labels -- `"aws_instance" "web"`: the type and the name it goes by.
(block (string_lit) @string.special)
(block (string_lit (template_literal) @string.special))

; Inside a template: `${ ... }` and `%{ if ... }` are code in a string.
(template_interpolation_start) @punctuation.special
(template_interpolation_end) @punctuation.special
(template_directive_start) @punctuation.special
(template_directive_end) @punctuation.special
(strip_marker) @punctuation.special

; Keywords of expressions and template directives.
[ "for" ] @keyword.repeat
[ "in" ] @keyword
[ "if" "else" "endif" ] @keyword.conditional
[ "endfor" ] @keyword.repeat
(for_cond "if" @keyword.conditional)

; Operators and punctuation.
[ "+" "-" "*" "/" "%" "==" "!=" "<" ">" "<=" ">=" "&&" "||" "!" "=" "=>" "?" ":" ] @operator
(ellipsis) @operator
[ "(" ")" "[" "]" "{" "}" ] @punctuation.bracket
[ "," "." ".*" "[*]" ] @punctuation.delimiter
(block_start) @punctuation.bracket
(block_end) @punctuation.bracket
(object_start) @punctuation.bracket
(object_end) @punctuation.bracket
(tuple_start) @punctuation.bracket
(tuple_end) @punctuation.bracket

(comment) @comment
