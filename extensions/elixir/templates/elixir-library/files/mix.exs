defmodule {{name_pascal}}.MixProject do
  use Mix.Project

  def project do
    [
      app: :{{name_snake}},
      version: "0.1.0",
      elixir: "~> 1.15",
      start_permanent: Mix.env() == :prod,
      deps: deps()
    ]
  end

  def application do
    [
      extra_applications: [:logger]
    ]
  end

  # Run "mix help deps" for what goes here.
  defp deps do
    []
  end
end
