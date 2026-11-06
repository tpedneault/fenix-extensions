(** What {{name_kebab}} does: greets whoever it's told to. *)

val greet : string list -> (string, string) result
(** A greeting for each name, together: [greet ["Ada"; "Grace"]] is
    [Ok "Hello, Ada and Grace!"]. *)
