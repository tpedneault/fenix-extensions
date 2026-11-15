; LaTeX, for tree-sitter-latex 0.6.0 (the grammar ships no queries; these are
; the pack's, in Fenix's capture names). A later pattern wins; a smaller
; range inside a larger one wins over it.

(line_comment) @comment
(block_comment) @comment
(comment_environment) @comment

; Commands and what they take
(command_name) @function
(generic_command command: (command_name) @function)
(placeholder) @parameter

; Sectioning
(part command: _ @keyword)
(chapter command: _ @keyword)
(section command: _ @keyword)
(subsection command: _ @keyword)
(subsubsection command: _ @keyword)
(paragraph command: _ @keyword)
(subparagraph command: _ @keyword)
(part text: (curly_group) @text.title)
(chapter text: (curly_group) @text.title)
(section text: (curly_group) @text.title)
(subsection text: (curly_group) @text.title)
(subsubsection text: (curly_group) @text.title)
(paragraph text: (curly_group) @text.title)
(subparagraph text: (curly_group) @text.title)

; Environments
(begin command: _ @keyword)
(end command: _ @keyword)
(begin name: (curly_group_text (text) @type))
(end name: (curly_group_text (text) @type))
(begin language: (curly_group_text (text) @string))

; Labels, references and citations
(label_definition command: _ @keyword)
(label_definition name: (curly_group_label (label) @label))
(label_reference command: _ @keyword)
(label_reference names: (curly_group_label_list (label) @label))
(label_reference_range command: _ @keyword)
(label_reference_range from: (curly_group_label (label) @label))
(label_reference_range to: (curly_group_label (label) @label))
(citation command: _ @keyword)
(citation keys: (curly_group_text_list (text) @label))

; Files and packages
(package_include command: _ @keyword)
(package_include paths: (curly_group_path_list (path) @string))
(class_include command: _ @keyword)
(class_include path: (curly_group_path (path) @string))
(latex_include command: _ @keyword)
(latex_include path: (curly_group_path (path) @string))
(graphics_include command: _ @keyword)
(graphics_include path: (curly_group_path (path) @string))
(bibtex_include command: _ @keyword)
(hyperlink command: _ @keyword)
(hyperlink uri: (curly_group_uri (uri) @text.uri))

; Definitions and the preamble's declarations
(new_command_definition command: _ @keyword)
(new_command_definition declaration: (curly_group_command_name (command_name) @function))
(new_command_definition declaration: (command_name) @function)
(environment_definition command: _ @keyword)
(environment_definition name: (curly_group_text (text) @type))
(title_declaration command: _ @keyword)
(author_declaration command: _ @keyword)
(caption command: _ @keyword)
(enum_item command: _ @keyword)

; Math
(inline_formula) @string.special
(displayed_equation) @string.special
(math_environment) @string.special
(subscript "_" @operator)
(superscript "^" @operator)
(operator) @operator

; Verbatim and code
(verbatim_environment verbatim: (comment) @text.literal)
(listing_environment) @text.literal
(minted_environment) @text.literal
(source_code) @text.literal

["{" "}"] @punctuation.bracket
["[" "]"] @punctuation.bracket
