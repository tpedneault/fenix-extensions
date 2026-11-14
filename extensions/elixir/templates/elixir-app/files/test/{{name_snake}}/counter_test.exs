defmodule {{name_pascal}}.CounterTest do
  use ExUnit.Case, async: true

  alias {{name_pascal}}.Counter

  setup do
    counter = start_supervised!({Counter, name: nil, initial: 10})
    %{counter: counter}
  end

  test "starts where it was told to", %{counter: counter} do
    assert Counter.value(counter) == 10
  end

  test "counts up", %{counter: counter} do
    assert Counter.increment(counter) == 11
    assert Counter.increment(counter) == 12
  end
end
