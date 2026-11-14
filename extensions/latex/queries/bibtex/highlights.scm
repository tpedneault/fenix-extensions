; BibTeX, for tree-sitter-bibtex at 8d04ed2: the repository's own queries
; (MIT), in Fenix's capture names.

(entry_type) @keyword
(string_type) @keyword
(preamble_type) @keyword
(comment) @comment
(junk) @comment

(field name: (identifier) @property)
(token (identifier) @parameter)
(string name: (identifier) @constant)

[(brace_word) (quote_word)] @string
(number) @number
[(key_brace) (key_paren)] @label
(command) @function

["=" "#"] @operator
["{" "}" "(" ")"] @punctuation.bracket
"," @punctuation.delimiter
