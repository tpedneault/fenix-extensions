; What folds in Scala, and the scopes SPC c s and the breadcrumbs list: each
; definition, the `class Fleet(...) extends Iterable[Station]:` or `def
; render[A](a: A)(using s: Show[A]): String =` that says what it is, and
; the block comments. Indentation syntax and braces alike. For
; tree-sitter-scala 0.24.0.

(class_definition) @fold
(object_definition) @fold
(trait_definition) @fold
(enum_definition) @fold
(package_object) @fold
(function_definition) @fold
(given_definition) @fold
(extension_definition) @fold
(block_comment) @fold
