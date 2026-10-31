--  Orbits: an orbit by its period, and a registry of them.
with Ada.Strings.Unbounded;
private with Ada.Containers.Vectors;

package Orbits
  with SPARK_Mode
is
   use Ada.Strings.Unbounded;

   Max_Orbits : constant := 16#40#;

   type Altitude is range 0 .. 100_000;
   subtype Low_Altitude is Altitude range 0 .. 2_000;

   type Status is (Nominal, Degraded, Lost);

   --  An orbit, by its period in minutes.
   type Orbit is record
      Name   : Unbounded_String;
      Period : Float := 92.68;
      Height : Altitude := 408;
   end record;

   type Tracker is tagged limited private;

   function Is_Low (O : Orbit) return Boolean is
     (O.Height in Low_Altitude);

   procedure Raise_By (H : in out Altitude; By : Altitude)
     with Global => null,
          Pre    => H <= Altitude'Last - By,
          Post   => H = H'Old + By;

   generic
      type Element is private;
      with function "<" (L, R : Element) return Boolean is <>;
   package Sorted is
      procedure Insert (E : Element);
   end Sorted;

private
   type Tracker is tagged limited record
      Count : Natural := 0;
   end record;
end Orbits;
