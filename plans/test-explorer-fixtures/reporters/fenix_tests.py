"""What the test explorer asks of pytest, as JSON lines on stdout, each
starting FENIX-TEST: each test collected (its id, file and line), and each
result as it comes (its outcome, how long it took, where it failed and why,
what it printed). Loaded with `-p fenix_tests` from the extension's /data."""
import json


def _say(kind, **fields):
    print("\nFENIX-TEST " + json.dumps(dict(kind=kind, **fields)), flush=True)


def pytest_collection_finish(session):
    for item in session.items:
        path, line, name = item.location
        _say("test", id=item.nodeid, file=path, line=(line or 0) + 1, name=name)


def pytest_collectreport(report):
    if report.failed:
        _say("collect-error", id=report.nodeid, message=str(report.longrepr)[-2000:])


def pytest_runtest_logreport(report):
    # setup, call, teardown: a failure in any is the test's; a pass only from call.
    if report.when != "call" and not report.failed and not report.skipped:
        return
    out = {"id": report.nodeid, "when": report.when, "outcome": report.outcome, "duration": round(report.duration, 6)}
    if hasattr(report, "wasxfail"):
        out["xfail"] = report.wasxfail or True
    crash = getattr(report.longrepr, "reprcrash", None)
    if crash is not None:
        out["file"], out["line"], out["message"] = crash.path, crash.lineno, crash.message
        # The deepest frame in the test's own file, not unittest's or a
        # library's: where the problem goes.
        test_file = report.location[0]
        for entry in getattr(getattr(report.longrepr, "reprtraceback", None), "reprentries", []):
            loc = getattr(entry, "reprfileloc", None)
            if loc is not None and loc.path.replace("\\", "/").endswith(test_file):
                out["at"] = [loc.path, loc.lineno]
    elif report.skipped and isinstance(report.longrepr, tuple):
        out["file"], out["line"], out["message"] = report.longrepr[0], report.longrepr[1], report.longrepr[2]
    if report.failed:
        out["text"] = report.longreprtext[-8000:]
    for name, text in report.sections:
        out.setdefault("output", {})[name] = text[-8000:]
    _say("result", **out)
