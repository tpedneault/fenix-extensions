with Ada.Text_IO; use Ada.Text_IO;

package body Orbits
  with SPARK_Mode
is
   procedure Raise_By (H : in out Altitude; By : Altitude) is
   begin
      H := H + By;
   end Raise_By;

   function Sum (A : Integer_Array) return Natural is
      Total : Natural := 0;
   begin
      for I in A'Range loop
         pragma Loop_Invariant (Total <= I * 100);
         Total := Total + A (I);
      end loop;
      return Total;
   end Sum;

   package body Sorted is
      procedure Insert (E : Element) is null;
   end Sorted;

   task type Beacon (Id : Positive) is
      entry Ping;
   end Beacon;

   task body Beacon is
   begin
      loop
         select
            accept Ping do
               Put_Line ("ping" & Id'Image);
            end Ping;
         or
            terminate;
         end select;
      end loop;
   end Beacon;

   protected type Counter is
      procedure Add;
      function Value return Natural;
   private
      N : Natural := 0;
   end Counter;

   protected body Counter is
      procedure Add is
      begin
         N := N + 1;
      end Add;

      function Value return Natural is (N);
   end Counter;

   procedure Report (S : Status) is
   begin
      case S is
         when Nominal =>
            Put_Line ("nominal");
         when Degraded | Lost =>
            if S = Lost then
               raise Program_Error with "lost";
            elsif S = Degraded then
               null;
            else
               Put_Line ('?' & "");
            end if;
      end case;
   exception
      when E : Constraint_Error =>
         Put_Line ("out of range");
   end Report;
end Orbits;
