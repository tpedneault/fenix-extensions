"""{{name}}: the app, made by create_app (Flask finds it: flask --app {{name_snake}})."""

from flask import Flask


def create_app(config: dict | None = None) -> Flask:
    app = Flask(__name__)
    app.config.from_prefixed_env()
    if config:
        app.config.update(config)

    from . import views

    app.register_blueprint(views.bp)
    return app


def main() -> None:
    """`uv run {{name_kebab}}`: the development server."""
    create_app().run(debug=True)
