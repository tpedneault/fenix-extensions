defmodule {{name_pascal}}.Counter do
  @moduledoc """
  A counter that lives in its own process.
  """

  use GenServer

  # Client

  def start_link(opts \\ []) do
    initial = Keyword.get(opts, :initial, 0)
    GenServer.start_link(__MODULE__, initial, name: Keyword.get(opts, :name, __MODULE__))
  end

  @doc "Adds one, and returns the new count."
  def increment(server \\ __MODULE__), do: GenServer.call(server, :increment)

  @doc "The count now."
  def value(server \\ __MODULE__), do: GenServer.call(server, :value)

  # Server

  @impl true
  def init(initial), do: {:ok, initial}

  @impl true
  def handle_call(:increment, _from, count), do: {:reply, count + 1, count + 1}
  def handle_call(:value, _from, count), do: {:reply, count, count}
end
