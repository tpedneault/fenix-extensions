{ pkgs, lib, ... }:
let
  x = 1;
  inherit (pkgs) git;
  y = pkgs.hello;
  z = { a = x; };
  foo = { bar = 1; };
in
{
  a = x == true;
  b = { inherit x; y = y; };
  c = {
    foo.bar = 1;
    foo.baz = 2;
  };
  d = [ git ];
  e = lib.mkIf true {
    z = z;
    f = foo;
  };
}
