(* The tests, on the standard library alone: each a name, what came of it
   and what should have. Add alcotest to the test's libraries for more. *)

let () =
  let results =
    [
      ("one name", Greeting.greet [ "Ada" ], Ok "Hello, Ada!");
      ("two names", Greeting.greet [ "Ada"; "Grace" ], Ok "Hello, Ada and Grace!");
      ("no names", Greeting.greet [], Error "whom to greet?");
    ]
  in
  let failed = List.filter (fun (_, got, want) -> got <> want) results in
  List.iter (fun (name, _, _) -> print_endline ("FAIL " ^ name)) failed;
  if failed <> [] then exit 1;
  Printf.printf "%d tests passed\n" (List.length results)
