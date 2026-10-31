# {{name}}

A [Django](https://www.djangoproject.com) site, managed by [uv](https://docs.astral.sh/uv/).

```
uv run python manage.py runserver   # http://127.0.0.1:8000
uv run python manage.py test        # or: uv run pytest
uv run python manage.py startapp <name>
```

Settings that differ between machines come from the environment:
`DJANGO_SECRET_KEY`, `DJANGO_DEBUG`, `DJANGO_ALLOWED_HOSTS`. `.env` holds
development's, and isn't committed.
