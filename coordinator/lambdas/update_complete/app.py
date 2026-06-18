"""
POST /api/updates/complete
Body: { "epoch": 42, "model_id": "fraud-detection-v2", "update_hash": "<64-char hex>" }
"""
import json
import re
import sys
import os
import time
import random
from datetime import datetime, timezone

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

from shared.audit import write_audit_entry
from shared.auth import AuthError, get_authenticated_org
from shared.dynamodb import put_item, query_gsi
from shared.response import bad_request, conflict, ok

HASH_RE = re.compile(r"^[0-9a-f]{64}$")


def _ulid() -> str:
    ts = int(time.time() * 1000)
    rand = random.getrandbits(64)
    return f"SUB#{ts:013x}{rand:016x}"


def _utc_now() -> str:
    return datetime.now(timezone.utc).isoformat()


def handler(event, context):
    try:
        org_id = get_authenticated_org(event)
    except AuthError as e:
        return e.response

    try:
        body = json.loads(event.get("body") or "{}")
    except json.JSONDecodeError:
        return bad_request("Request body must be valid JSON")

    epoch_number = body.get("epoch_number") or body.get("epoch")
    model_id = (body.get("model_id") or "").strip()
    update_hash = (body.get("update_hash") or "").strip()

    if epoch_number is None or not model_id:
        return bad_request("epoch and model_id are required")

    if not HASH_RE.match(update_hash):
        return bad_request("update_hash must be a 64-character lowercase hex SHA-256 string")

    epoch_id = f"EPOCH#{model_id}#{epoch_number}"

    # Check for duplicate submission (Property 3 — idempotent trigger relies on this)
    existing = query_gsi(
        "SUBMISSION_TABLE",
        "epoch_id-org_id-index",
        pk_name="epoch_id",
        pk_value=epoch_id,
        sk_name="org_id",
        sk_value=org_id,
        limit=1,
    )
    if existing:
        return conflict("Organization has already submitted for this epoch")

    # Deterministic S3 key — matches update_url exactly (Bug 1 fix)
    s3_key = f"updates/{model_id}/{epoch_number}/{org_id}/update.bin"

    submission_id = _ulid()
    item = {
        "submission_id": submission_id,
        "epoch_id": epoch_id,
        "org_id": org_id,
        "model_id": model_id,
        "epoch_number": int(epoch_number),
        "update_hash": update_hash,
        "s3_key": s3_key,
        "submitted_at": _utc_now(),
        "status": "RECEIVED",
    }
    put_item("SUBMISSION_TABLE", item)

    # Non-blocking audit entry
    write_audit_entry(
        model_id=model_id,
        epoch_number=int(epoch_number),
        event_type="UPDATE_SUBMITTED",
        org_id=org_id,
        payload=json.dumps({"update_hash": update_hash, "s3_key": s3_key}),
    )

    return ok({"status": "recorded", "submission_id": submission_id})
