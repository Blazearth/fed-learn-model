"""
POST /api/models/download-url
Body: { "model_id": "...", "model_version": "..." }
Returns: { "url": "<pre-signed S3 GET URL>" }
"""
import json
import sys
import os

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

from shared.auth import AuthError, get_authenticated_org
from shared.response import bad_request, not_found, ok
from shared.s3 import generate_presigned_get


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
    model_version = body.get("model_version", "").strip()
    if not model_id or not model_version:
        return bad_request("model_id and model_version are required")

    bucket = os.environ["BUCKET_NAME"]
    key = f"models/{model_id}/{model_version}/model.npy"

    presigned_url = generate_presigned_get(bucket, key, expiry=900)
    return ok({"url": presigned_url, "download_url": presigned_url})
