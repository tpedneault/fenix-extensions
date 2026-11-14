defmodule Orbit do
  @moduledoc "Orbits, kept and told."

  @doc """
  The period of an orbit.

      iex> Orbit.period(5520)
      5520
  """
  def period(radius) do
    unused = 1
    radius
  end

  def describe(name, crew) do
    "#{name}: #{crew}"
  end
  def helper(x), do: Enum.map( [1,2,x], fn y -> y*2 end )

  defp nothing, do: :ok

  def call do
    Orbit.Missing.run()
  end
end
