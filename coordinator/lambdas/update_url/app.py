"""
POST /api/updates/upload-url
Body: { "model_id": "...", "epoch_number": 42 }
Returns: { "url": "<pre-signed S3 PUT URL>" }

CRITICAL: S3 key is fully deterministic — same formula used in update_complete.
Key: updates/{model_id}/{epoch_number}/{org_id}/update.bin
"""
import json
import sys
import os

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

from shared.auth import AuthError, get_authenticated_org
from shared.response import bad_request, ok
from shared.s3 import generate_presigned_put


def handler(event, context):
    try:
        org_id = get_authenticated_org(event)
    except AuthError as e:
        return e.response

    try:
        body = json.loads(event.get("body") or "{}")
    except json.JSONDecodeError:
        return bad_request("Request body must be valid JSON")

    model_id = body.get("model_id", "").strip()
    epoch_number = body.get("epoch_number")
    if not model_id or epoch_number is None:
        return bad_request("model_id and epoch_number are required")

    bucket = os.environ["BUCKET_NAME"]
    # Deterministic key — must match update_complete exactly
    key = f"updates/{model_id}/{epoch_number}/{org_id}/update.bin"

    presigned_url = generate_presigned_put(bucket, key, expiry=1800)
    return ok({"url": presigned_url, "upload_url": presigned_url})
