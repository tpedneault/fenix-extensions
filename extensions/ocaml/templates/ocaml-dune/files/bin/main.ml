(* {{name_kebab}}: greets whoever it's told to. *)

let () =
  let names = List.tl (Array.to_list Sys.argv) in
  match Greeting.greet names with
  | Ok line -> print_endline line
  | Error why ->
      prerr_endline ("{{name_kebab}}: " ^ why);
      exit 2
