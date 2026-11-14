{ pkgs, lib, unused, ... }:
let
  x = 1;
  y = 2;
  config = { a = 1; };
  eta = lib.map (v: toString v) [ 1 2 ];
  b = if true then true else false;
  c = { foo.bar = 1; foo.baz = 2; };
  d = (import ./foo.nix) { };
  e = pkgs.stdenv.mkDerivation rec { name = "x"; };
  ll = { inherit x; };
  bad = { a = 1; }.a;
  rr = rec { a = 1; };
  m = lib.id (lib.mkIf (x == 1) { });
  l = [ (1) ];
  k = let in 3;
in
{ environment.systemPackages = with pkgs; [ git ]; inherit config; q = d; }
