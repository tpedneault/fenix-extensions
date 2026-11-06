; Enter indents a level inside a body that opens on the cursor's line: a
; message's, an enum's, a service's, a oneof's, an rpc's options, an
; option's aggregate value.
[
  (message_body)
  (enum_body)
  (oneof)
  (service)
  (block_lit)
] @indent

; An rpc's body is optional (it may end in ";"): the brace, not the rpc.
(rpc
  "{" @indent)

(extend) @indent
