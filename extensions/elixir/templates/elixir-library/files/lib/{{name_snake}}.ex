defmodule {{name_pascal}} do
  @moduledoc """
  What {{name}} is for.
  """

  @doc """
  Adds up the lengths of `words`.

  ## Examples

      iex> {{name_pascal}}.letters(["Ada", "Grace", "Rob"])
      11

      iex> {{name_pascal}}.letters([])
      0

  """
  @spec letters([String.t()]) :: non_neg_integer()
  def letters(words) do
    words
    |> Enum.map(&String.length/1)
    |> Enum.sum()
  end
end
