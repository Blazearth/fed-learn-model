"""
Flask server wrapping the Lambda handlers for local Docker Compose development.
Translates HTTP requests → Lambda event format → Lambda handler → HTTP response.
"""
import json
import os
import sys

from flask import Flask, request, jsonify

# Add lambdas/ and aggregation/ to path
sys.path.insert(0, os.path.join(os.path.dirname(__file__), "lambdas"))
sys.path.insert(0, os.path.join(os.path.dirname(__file__), "aggregation"))

app = Flask(__name__)


def _build_event(req, path_params=None):
    """Convert Flask request to API Gateway-style event dict."""
    try:
        body = req.get_data(as_text=True) or None
    except Exception:
        body = None
    return {
        "headers": dict(req.headers),
        "queryStringParameters": dict(req.args) or {},
        "pathParameters": path_params or {},
        "body": body,
        "requestContext": {"http": {"method": req.method}},
    }


def _flask_response(lambda_resp):
    """Convert Lambda response dict to Flask response."""
    status  = lambda_resp.get("statusCode", 200)
    headers = lambda_resp.get("headers", {})
    body    = lambda_resp.get("body", "")
    resp = app.response_class(
        response=body,
        status=status,
        mimetype=headers.get("Content-Type", "application/json"),
    )
    return resp


# ── Routes ────────────────────────────────────────────────────────────────────

@app.route("/api/health", methods=["GET"])
def health():
    from health.app import handler
    return _flask_response(handler(_build_event(request), {}))


@app.route("/api/epochs/active", methods=["GET"])
def epochs_active():
    from epoch_query.app import handler
    return _flask_response(handler(_build_event(request), {}))


@app.route("/api/models/download-url", methods=["POST"])
def model_download_url():
    from model_url.app import handler
    return _flask_response(handler(_build_event(request), {}))


@app.route("/api/updates/upload-url", methods=["POST"])
def update_upload_url():
    from update_url.app import handler
    return _flask_response(handler(_build_event(request), {}))


@app.route("/api/updates/complete", methods=["POST"])
def update_complete():
    from update_complete.app import handler
    return _flask_response(handler(_build_event(request), {}))


@app.route("/api/audit", methods=["GET"])
def audit():
    from audit_query.app import handler
    return _flask_response(handler(_build_event(request), {}))


if __name__ == "__main__":
    app.run(host="0.0.0.0", port=8080, debug=True)
