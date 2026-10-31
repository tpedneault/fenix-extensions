; Where Enter indents a level more in Ruby: after a line that opens a
; `def`, a `class`, a block, an `if`, a loop, a `begin`, and after its
; `else`, `when` and `rescue`. For tree-sitter-ruby 0.23.1.
;
; Fenix indents after a line where one of these starts and doesn't end
; before the cursor. Each is one with its `end` (an endless `def x = 1`
; ends on its line; `x while y` is a `while_modifier`), and brackets
; aren't here: `x = [1, 2]` and `.map { |x| x * 2 }` end where Enter is
; pressed. A `when x then y` on one line does too, and indents.

(class "end") @indent
(module "end") @indent
(singleton_class "end") @indent
(method "end") @indent
(singleton_method "end") @indent
(do_block "end") @indent
(if "end") @indent
(unless "end") @indent
(case "end") @indent
(case_match "end") @indent
(while) @indent
(until) @indent
(for) @indent
(begin "end") @indent

[
  (elsif)
  (else)
  (when)
  (in_clause)
  (rescue)
  (ensure)
] @indent
