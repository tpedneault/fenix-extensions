defmodule {{name_pascal}}Test do
  use ExUnit.Case, async: true

  doctest {{name_pascal}}

  describe "letters/1" do
    test "counts characters, not bytes" do
      assert {{name_pascal}}.letters(["Zoë"]) == 3
    end
  end
end
