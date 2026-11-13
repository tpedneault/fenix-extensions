defmodule Orbit.Crew do
  @moduledoc """
  The people aboard, kept in a process of their own.
  """

  use GenServer

  alias Orbit.Crew.Member

  @max_crew 7
  @type roster :: [Member.t()]

  # Client

  @doc "Starts the roster, empty."
  @spec start_link(keyword()) :: GenServer.on_start()
  def start_link(opts \\ []) do
    GenServer.start_link(__MODULE__, %{}, name: Keyword.get(opts, :name, __MODULE__))
  end

  def board(server \\ __MODULE__, name) when is_binary(name) do
    GenServer.call(server, {:board, name})
  end

  # Server

  @impl true
  def init(state), do: {:ok, state}

  @impl true
  def handle_call({:board, name}, _from, state) do
    with {:ok, clean} <- validate(name),
         false <- Map.has_key?(state, clean) do
      {:reply, :ok, Map.put(state, clean, 1_000.5)}
    else
      true -> {:reply, {:error, :aboard}, state}
      {:error, reason} -> {:reply, {:error, reason}, state}
    end
  end

  defp validate(name) do
    case String.trim(name) do
      "" -> {:error, :blank}
      <<first, _::binary>> = clean when first != ?\s -> {:ok, clean}
    end
  end

  def roster(state) do
    state
    |> Map.keys()
    |> Enum.map(&String.upcase/1)
    |> Enum.sort_by(fn name -> {String.length(name), name} end)
    |> tap(&IO.inspect(&1, label: "roster"))
  end

  def pattern, do: ~r/^\w+$/i
  def banner(name), do: "Welcome aboard, #{name}!\n"
end
