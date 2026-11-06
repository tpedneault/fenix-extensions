(** Orbits, kept by name. *)

open Printf

(* Two at once. *)
type kind = Leo | Meo | Geo [@@deriving show]

type orbit = {
  name : string;
  period : float;
  mutable crew : int;
}

exception Too_many of int

let max_crew = 7

module Registry = struct
  module M = Map.Make (String)

  type t = orbit M.t

  let empty : t = M.empty

  let add (o : orbit) (known : t) =
    if o.crew > max_crew then raise (Too_many o.crew)
    else M.add o.name o known
end

module type TRACKER = sig
  val track : orbit -> unit
end

let kind_of altitude =
  match altitude with
  | a when a < 2000. -> Leo
  | a when a < 35786. -> Meo
  | _ -> Geo

let describe ?(prefix = "") ~unit o =
  sprintf "%s%s every %.1f%s\n" prefix o.name o.period unit

class tracker name = object (self)
  val mutable seen = 0
  method name = name
  method see = seen <- seen + 1; self#name
end

let rec count = function
  | [] -> 0
  | _ :: rest -> 1 + count rest

let () =
  let iss = { name = "ISS"; period = 5520.; crew = 3 } in
  let ratio = 1.5e3 in
  for i = 1 to 3 do
    print_endline (describe ~unit:"s" iss)
  done;
  while false do () done;
  try ignore (Registry.add iss Registry.empty) with Too_many n -> failwith (string_of_int n);
  [ 1; 2; 3 ] |> List.map (fun x -> x * 2) |> List.iter (printf "%d %f %b %c\n" 0x2A ratio true 'x')
