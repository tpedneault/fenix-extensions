# {{name}}

A [Flask](https://flask.palletsprojects.com) app, managed by [uv](https://docs.astral.sh/uv/).

```
uv run flask --app {{name_snake}} run --debug   # http://127.0.0.1:5000
uv run pytest
```

`FLASK_`-prefixed variables (`FLASK_SECRET_KEY`) are its config; `.env`
holds development's, and isn't committed.
