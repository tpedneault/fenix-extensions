let greet = function
  | [] -> Error "whom to greet?"
  | names -> Ok ("Hello, " ^ String.concat " and " names ^ "!")
