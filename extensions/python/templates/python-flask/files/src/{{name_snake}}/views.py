from flask import Blueprint, jsonify, render_template

bp = Blueprint("views", __name__)


@bp.get("/")
def home() -> str:
    return render_template("home.html", name="{{name}}")


@bp.get("/health")
def health():
    return jsonify(status="ok")
