"""
Multi-Krum Byzantine-resilient aggregation worker.
Runs as ECS Fargate task (production) or in-process via LOCAL_MODE.

Reference: Blanchard et al., "Machine Learning with Adversaries:
Byzantine Tolerant Gradient Descent" (NeurIPS 2017)

Bug 3 fix applied: selection set size is n-f-2, NOT f+1.
"""
import base64
import hashlib
import io
import json
import logging
import os
from datetime import datetime, timezone

import boto3
import numpy as np
from cryptography.hazmat.primitives.serialization import load_pem_private_key

logging.basicConfig(level=logging.INFO)
logger = logging.getLogger(__name__)


# ── Multi-Krum ────────────────────────────────────────────────────────────────

class MultiKrumAggregator:
    def aggregate(self, updates: list, f: int | None = None) -> np.ndarray:
        """
        Aggregate a list of 1-D numpy arrays using Multi-Krum.

        Args:
            updates: list of np.ndarray (each already flattened to 1-D)
            f: number of Byzantine workers to tolerate.
               Defaults to floor((n-2)/2) which tolerates up to 33%.

        Returns:
            Aggregated 1-D numpy array.
        """
        n = len(updates)
        if n == 0:
            raise ValueError("No updates to aggregate")

        if f is None:
            f = (n - 2) // 2

        # Minimum participants for Multi-Krum: n >= 2f+3
        if n < 2 * f + 3:
            logger.warning(
                "n=%d < 2f+3=%d — falling back to FedAvg", n, 2 * f + 3
            )
            return np.mean(updates, axis=0)

        matrix = np.stack([u.flatten() for u in updates])   # (n, d)

        # Pairwise squared Euclidean distances: (n, n)
        diff = matrix[:, np.newaxis, :] - matrix[np.newaxis, :, :]
        dist = np.sum(diff ** 2, axis=-1)

        # Score each update: sum distances to k=n-f-2 nearest neighbours
        k = n - f - 2
        scores = np.zeros(n)
        for i in range(n):
            row = dist[i].copy()
            row[i] = np.inf           # exclude self
            scores[i] = np.sum(np.sort(row)[:k])

        # BUG 3 FIX: select n-f-2 updates (NOT f+1) to maximise honest participation
        num_to_select = n - f - 2
        selected = np.argsort(scores)[:num_to_select]

        logger.info(
            "Multi-Krum: n=%d f=%d k=%d selected=%d indices=%s",
            n, f, k, num_to_select, selected.tolist(),
        )
        return np.mean(matrix[selected], axis=0)


# ── Aggregation entry point ───────────────────────────────────────────────────

def run_aggregation(epoch_id: str | None = None, model_id: str | None = None) -> None:
    epoch_id = epoch_id or os.environ["EPOCH_ID"]
    model_id = model_id or os.environ["MODEL_ID"]
    bucket = os.environ["BUCKET_NAME"]

    ddb_kwargs = {}
    s3_kwargs = {}
    endpoint = os.environ.get("DYNAMODB_ENDPOINT")
    s3_endpoint = os.environ.get("S3_ENDPOINT")
    region = os.environ.get("AWS_DEFAULT_REGION", "us-east-1")

    if endpoint:
        ddb_kwargs["endpoint_url"] = endpoint
    if s3_endpoint:
        s3_kwargs["endpoint_url"] = s3_endpoint
        s3_kwargs["config"] = boto3.session.Config(signature_version="s3v4")

    ddb = boto3.resource("dynamodb", region_name=region, **ddb_kwargs)
    s3 = boto3.client("s3", region_name=region, **s3_kwargs)

    submission_table = ddb.Table(os.environ["SUBMISSION_TABLE"])
    epoch_table = ddb.Table(os.environ["EPOCH_TABLE"])
    audit_table = ddb.Table(os.environ["AUDIT_TABLE"])

    # Guard: skip if epoch was already completed (prevents manual reruns corrupting history)
    epoch_check = epoch_table.get_item(Key={"epoch_id": epoch_id}).get("Item", {})
    if epoch_check.get("status") == "COMPLETED":
        logger.warning(
            "epoch=%s already COMPLETED — skipping to prevent history corruption", epoch_id
        )
        return

    # 1. Fetch submissions — only RECEIVED status (not REJECTED/CORRUPTED/FLAGGED)
    resp = submission_table.query(
        IndexName="epoch_id-org_id-index",
        KeyConditionExpression=boto3.dynamodb.conditions.Key("epoch_id").eq(epoch_id),
    )
    all_submissions = resp["Items"]
    submissions = [s for s in all_submissions if s.get("status", "RECEIVED") == "RECEIVED"]
    logger.info(
        "epoch=%s Fetched %d submissions (%d RECEIVED, %d skipped)",
        epoch_id, len(all_submissions), len(submissions), len(all_submissions) - len(submissions),
    )

    if not submissions:
        logger.error("No submissions found for %s", epoch_id)
        _fail_epoch(epoch_table, audit_table, epoch_id, model_id, "No submissions found")
        return

    # Require minimum 2 participants before aggregation.
    # Insufficient count is a retryable wait condition — do NOT mark epoch FAILED.
    # A late-arriving participant should still be able to contribute.
    threshold = int(
        (epoch_table.get_item(Key={"epoch_id": epoch_id})["Item"]
         .get("secure_agg_threshold", 2))
    )
    required = max(2, threshold)
    if len(submissions) < required:
        logger.info(
            "epoch=%s waiting for more submissions (%d/%d) — not failing, will retry on next submission",
            epoch_id, len(submissions), required,
        )
        return

    # 2. Download updates from S3 — files are .npy format (saved with np.save)
    # Fix 4: re-verify SHA-256 hash against stored value (defense in depth)
    # Fix 3: reject updates whose shape differs from the first valid update
    updates = []
    expected_shape = None
    for sub in submissions:
        try:
            obj = s3.get_object(Bucket=bucket, Key=sub["s3_key"])
            data = obj["Body"].read()

            # Fix 4: verify hash matches what was recorded at submission time
            real_hash = hashlib.sha256(data).hexdigest()
            stored_hash = sub.get("update_hash", "")
            if stored_hash and real_hash != stored_hash:
                logger.warning(
                    "epoch=%s Hash mismatch for %s: stored=%s actual=%s — skipping",
                    epoch_id, sub["s3_key"], stored_hash[:16], real_hash[:16],
                )
                continue

            arr = np.load(io.BytesIO(data)).flatten().astype(np.float32)

            # Fix 3: enforce consistent update shape across all participants
            if expected_shape is None:
                expected_shape = arr.shape
            elif arr.shape != expected_shape:
                logger.warning(
                    "epoch=%s Shape mismatch for org=%s: expected=%s got=%s — skipping",
                    epoch_id, sub.get("org_id"), expected_shape, arr.shape,
                )
                continue

            updates.append(arr)
            logger.info(
                "epoch=%s Loaded update org=%s shape=%s",
                epoch_id, sub.get("org_id"), arr.shape,
            )
        except Exception as exc:
            logger.warning("epoch=%s Could not download/load %s: %s", epoch_id, sub["s3_key"], exc)

    if not updates:
        _fail_epoch(epoch_table, audit_table, epoch_id, model_id, "All downloads failed")
        _release_lock(epoch_table, model_id)
        return

    # 3. Run Multi-Krum
    aggregator = MultiKrumAggregator()
    aggregate = aggregator.aggregate(updates)

    # 4. Serialize to .npy bytes
    buf = io.BytesIO()
    np.save(buf, aggregate)
    model_bytes = buf.getvalue()

    # 5. Sign with Ed25519 (from SSM or local key file)
    signing_key_pem = _load_signing_key()
    private_key = load_pem_private_key(signing_key_pem, password=None)
    signature = private_key.sign(model_bytes)
    model_hash = hashlib.sha256(model_bytes).hexdigest()

    # 6. Determine new version
    epoch = epoch_table.get_item(Key={"epoch_id": epoch_id})["Item"]
    new_epoch_number = int(epoch["epoch_number"]) + 1
    new_version = f"v{new_epoch_number}"
    s3_key = f"models/{model_id}/{new_version}/model.npy"

    # 7. Upload new model to S3
    s3.put_object(Bucket=bucket, Key=s3_key, Body=model_bytes)
    logger.info("epoch=%s Uploaded new model: %s hash=%s", epoch_id, s3_key, model_hash)

    # 8. Mark current epoch COMPLETED
    epoch_table.update_item(
        Key={"epoch_id": epoch_id},
        UpdateExpression="SET #s = :done, completed_at = :now",
        ExpressionAttributeNames={"#s": "status"},
        ExpressionAttributeValues={
            ":done": "COMPLETED",
            ":now": datetime.now(timezone.utc).isoformat(),
        },
    )

    # 9. Create next epoch as PENDING
    next_epoch_id = f"EPOCH#{model_id}#{new_epoch_number}"
    epoch_table.put_item(Item={
        "epoch_id": next_epoch_id,
        "model_id": model_id,
        "epoch_number": new_epoch_number,
        "status": "PENDING",
        "model_version": new_version,
        "model_hash": model_hash,
        "model_s3_key": s3_key,
        "model_signature": base64.b64encode(signature).decode(),
        "architecture_hash": epoch.get("architecture_hash", ""),
        "fedprox_mu": epoch.get("fedprox_mu", "0.01"),
        "privacy_epsilon": epoch.get("privacy_epsilon", "1.0"),
        "privacy_delta": epoch.get("privacy_delta", "0.00001"),
        "secure_agg_threshold": epoch.get("secure_agg_threshold", 1),
        "drift_alerts": "[]",
        "dataset_schema": epoch.get("dataset_schema", "null"),
        "created_at": datetime.now(timezone.utc).isoformat(),
    })

    # 10. Write audit entry
    _write_audit(audit_table, model_id, new_epoch_number, "MODEL_PUBLISHED",
                 json.dumps({"model_hash": model_hash, "s3_key": s3_key,
                              "updates_used": len(updates)}))

    # 11. Delete epoch lock on success (Bug 2 fix: release lock so next epoch can be activated)
    _release_lock(epoch_table, model_id)

    # 12. Publish CloudWatch success metric
    _put_metric("AggregationSuccess", 1, model_id=model_id)

    logger.info("epoch=%s Aggregation complete. Next epoch: %s version=%s", epoch_id, next_epoch_id, new_version)


def _put_metric(metric_name: str, value: float, model_id: str = "") -> None:
    """Publish a single CloudWatch metric to the FederatedLearning namespace."""
    try:
        cw = boto3.client(
            "cloudwatch",
            region_name=os.environ.get("AWS_DEFAULT_REGION", "us-east-1"),
        )
        dimensions = [{"Name": "ModelId", "Value": model_id}] if model_id else []
        cw.put_metric_data(
            Namespace="FederatedLearning",
            MetricData=[{
                "MetricName": metric_name,
                "Value": value,
                "Unit": "Count",
                "Dimensions": dimensions,
            }],
        )
    except Exception as exc:
        logger.warning("CloudWatch metric '%s' failed (non-blocking): %s", metric_name, exc)


def _release_lock(epoch_table, model_id: str) -> None:
    """Delete the single-active-epoch lock item so the next epoch can be activated."""
    try:
        epoch_table.delete_item(Key={"epoch_id": f"MODEL#{model_id}#LOCK"})
        logger.info("Released epoch lock for model_id=%s", model_id)
    except Exception as exc:
        logger.warning(
            "Could not release epoch lock for model_id=%s: %s — "
            "manual cleanup required: delete PK=MODEL#%s#LOCK from EpochTable",
            model_id, exc, model_id,
        )


def _fail_epoch(epoch_table, audit_table, epoch_id: str, model_id: str, reason: str) -> None:
    logger.error("epoch=%s FAILED reason=%s", epoch_id, reason)
    try:
        epoch_table.update_item(
            Key={"epoch_id": epoch_id},
            UpdateExpression="SET #s = :failed",
            ExpressionAttributeNames={"#s": "status"},
            ExpressionAttributeValues={":failed": "FAILED"},
        )
    except Exception as exc:
        logger.error("Could not mark epoch %s as FAILED: %s", epoch_id, exc)
    # Bug 12 fix: always release lock on failure so the next epoch can be activated
    _release_lock(epoch_table, model_id)
    _put_metric("AggregationFailure", 1, model_id=model_id)
    _write_audit(audit_table, model_id, 0, "AGGREGATION_FAILED",
                 json.dumps({"reason": reason, "epoch_id": epoch_id}))


def _write_audit(table, model_id: str, epoch_number: int, event_type: str, payload: str) -> None:
    """
    Write one audit entry with proper hash chaining.
    Bug 13 fix: reads the previous entry hash instead of always using '0'*64.
    """
    import time, random, hashlib
    from boto3.dynamodb.conditions import Key as DKey

    try:
        # 1. Read most recent entry for this model_id to get previous_hash
        previous_hash = "0" * 64
        resp = table.query(
            IndexName="model_id-created_at-index",
            KeyConditionExpression=DKey("model_id").eq(model_id),
            ScanIndexForward=False,
            Limit=1,
        )
        items = resp.get("Items", [])
        if items:
            previous_hash = items[0].get("entry_hash", "0" * 64)

        # 2. Build and hash the entry
        ts = int(time.time() * 1000)
        rand = random.getrandbits(64)
        entry_id = f"AUDIT#{ts:013x}{rand:016x}"
        created_at = datetime.now(timezone.utc).isoformat()
        raw = f"{entry_id}{event_type}SYSTEM{payload}{previous_hash}"
        entry_hash = hashlib.sha256(raw.encode()).hexdigest()

        table.put_item(Item={
            "entry_id": entry_id,
            "model_id": model_id,
            "epoch_number": epoch_number,
            "event_type": event_type,
            "org_id": "SYSTEM",
            "payload": payload,
            "previous_hash": previous_hash,
            "entry_hash": entry_hash,
            "created_at": created_at,
        })
        logger.info("epoch=%s audit entry written: %s", epoch_number, event_type)
    except Exception as exc:
        logger.error("audit write failed (non-blocking): %s", exc)


def _load_signing_key() -> bytes:
    """Load Ed25519 private key PEM from SSM (prod) or local file (dev)."""
    key_file = os.path.join(os.path.dirname(__file__), "..", "signing_key", "private.pem")
    if os.path.exists(key_file):
        with open(key_file, "rb") as f:
            return f.read()
    # Production: fetch from SSM
    ssm = boto3.client("ssm", region_name=os.environ.get("AWS_DEFAULT_REGION", "us-east-1"))
    resp = ssm.get_parameter(Name="/fl-coordinator/ed25519-private-key", WithDecryption=True)
    return resp["Parameter"]["Value"].encode()


if __name__ == "__main__":
    run_aggregation()
