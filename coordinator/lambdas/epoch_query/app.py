"""
GET /api/epochs/active?model_id={model_id}
Returns EpochMetadata JSON matching the Rust daemon's types.rs::EpochMetadata struct.
"""
import json
import sys
import os

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

from shared.auth import AuthError, get_authenticated_org
from shared.dynamodb import get_item, query_gsi
from shared.response import bad_request, not_found, ok


def handler(event, context):
    # 1. Authenticate
    try:
        org_id = get_authenticated_org(event)
    except AuthError as e:
        return e.response

    # 2. Validate query param
    params = event.get("queryStringParameters") or {}
    model_id = params.get("model_id", "").strip()
    if not model_id:
        return bad_request("model_id query parameter is required")

    # 3. Find ACTIVE epoch for this model_id using GSI
    items = query_gsi(
        "EPOCH_TABLE",
        "model_id-status-index",
        pk_name="model_id",
        pk_value=model_id,
        sk_name="status",
        sk_value="ACTIVE",
        limit=1,
    )
    if not items:
        return not_found(f"No active epoch for model_id '{model_id}'")

    epoch = items[0]

    # 4. Build secure_agg_participants from all ACTIVE orgs
    all_orgs = query_gsi(
        "ORG_TABLE",
        "status-index",
        pk_name="status",
        pk_value="ACTIVE",
    ) if _gsi_exists() else _scan_active_orgs()

    participants = [
        {"org_id": o["org_id"], "public_key": o.get("public_key", "")}
        for o in all_orgs
    ]

    # 5. Return EpochMetadata — field names exactly match Rust daemon's struct
    metadata = {
        "epoch_number": int(epoch["epoch_number"]),
        "model_id": epoch["model_id"],
        "model_version": epoch["model_version"],
        "model_hash": epoch["model_hash"],
        "model_signature": epoch.get("model_signature", ""),
        "architecture_hash": epoch.get("architecture_hash", ""),
        "fedprox_mu": float(epoch.get("fedprox_mu", 0.01)),
        "privacy_epsilon": float(epoch.get("privacy_epsilon", 1.0)),
        "privacy_delta": float(epoch.get("privacy_delta", 1e-5)),
        "secure_agg_participants": participants,
        "secure_agg_threshold": int(epoch.get("secure_agg_threshold", 1)),
        "drift_alerts": json.loads(epoch.get("drift_alerts", "[]")),
        "dataset_schema": json.loads(epoch.get("dataset_schema", "null")),
    }

    return ok(metadata)


def _gsi_exists() -> bool:
    """OrgTable has no GSI in local mode — fall back to scan."""
    return False


def _scan_active_orgs() -> list:
    """Scan OrgTable for ACTIVE orgs (used in local mode without GSI)."""
    import boto3
    import os
    kwargs = {}
    endpoint = os.environ.get("DYNAMODB_ENDPOINT")
    if endpoint:
        kwargs["endpoint_url"] = endpoint
    ddb = boto3.resource(
        "dynamodb",
        region_name=os.environ.get("AWS_DEFAULT_REGION", "us-east-1"),
        **kwargs,
    )
    table = ddb.Table(os.environ["ORG_TABLE"])
    resp = table.scan(
        FilterExpression="#s = :active",
        ExpressionAttributeNames={"#s": "status"},
        ExpressionAttributeValues={":active": "ACTIVE"},
    )
    return resp.get("Items", [])
