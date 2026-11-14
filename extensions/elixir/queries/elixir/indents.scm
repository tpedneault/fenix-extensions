; Where Enter indents a level more in Elixir: inside a `do` block, an
; anonymous function, a map, list, tuple or bitstring, a call's arguments.
; For tree-sitter-elixir 0.3.5.

[
  (do_block)
  (anonymous_function)
  (map)
  (list)
  (tuple)
  (bitstring)
  (arguments)
] @indent
