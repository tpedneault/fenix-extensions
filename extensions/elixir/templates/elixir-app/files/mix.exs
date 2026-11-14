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

  # The application starts {{name_pascal}}.Application, whose supervisor
  # starts what lib/{{name_snake}}/application.ex lists.
  def application do
    [
      extra_applications: [:logger],
      mod: {
        {{name_pascal}}.Application,
        []
      }
    ]
  end

  # Run "mix help deps" for what goes here.
  defp deps do
    []
  end
end
