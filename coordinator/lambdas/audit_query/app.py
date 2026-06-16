"""GET /api/audit?model_id={model_id}&limit={n}"""
import sys
import os

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

from shared.auth import AuthError, get_authenticated_org
from shared.dynamodb import query_gsi
from shared.response import bad_request, ok


def handler(event, context):
    try:
        get_authenticated_org(event)
    except AuthError as e:
        return e.response

    params = event.get("queryStringParameters") or {}
    model_id = params.get("model_id", "").strip()
    if not model_id:
        return bad_request("model_id query parameter is required")

    try:
        limit = min(int(params.get("limit", 20)), 100)
    except (ValueError, TypeError):
        limit = 20

    entries = query_gsi(
        "AUDIT_TABLE",
        "model_id-created_at-index",
        pk_name="model_id",
        pk_value=model_id,
        scan_forward=False,
        limit=limit,
    )
    return ok({"entries": entries, "count": len(entries)})
