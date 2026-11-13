; What folds in Elixir, and the scopes SPC c s and the breadcrumbs list:
; the definitions (a module, a function, a test) by their `do` block, and
; the blocks and collections that span lines. For tree-sitter-elixir 0.3.5.

((call
  target: (identifier) @form) @fold
  (#any-of? @form "defmodule" "defprotocol" "defimpl" "def" "defp" "defmacro" "defmacrop" "defguard" "defguardp" "defdelegate" "test" "describe" "setup" "setup_all" "property"))

(do_block) @fold
(anonymous_function) @fold
(map) @fold
(list) @fold
(tuple) @fold
(bitstring) @fold
(sigil) @fold
