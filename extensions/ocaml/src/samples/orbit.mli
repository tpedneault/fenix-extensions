(** Orbits, kept by name. *)

type kind = Leo | Meo | Geo

type orbit = {
  name : string;
  period : float;
  mutable crew : int;
}

exception Too_many of int

val max_crew : int

module Registry : sig
  type t

  val empty : t
  val add : orbit -> t -> t
end

val kind_of : float -> kind
val describe : ?prefix:string -> unit:string -> orbit -> string
