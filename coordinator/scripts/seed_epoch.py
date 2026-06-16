#!/usr/bin/env python3
"""
Seed an initial PENDING epoch into EpochTable and upload a placeholder model to S3.

Usage:
  python scripts/seed_epoch.py --model-id fraud-detection-v2 --epoch-number 1 \
      --threshold 2 --local
"""
import argparse
import base64
import hashlib
import io
import os
from datetime import datetime, timezone

import boto3
import numpy as np


def _ddb(local: bool):
    kwargs = {"region_name": "us-east-1"}
    if local:
        kwargs["endpoint_url"] = os.environ.get("DYNAMODB_ENDPOINT", "http://localhost:8000")
        os.environ.setdefault("AWS_ACCESS_KEY_ID", "test")
        os.environ.setdefault("AWS_SECRET_ACCESS_KEY", "test")
    return boto3.resource("dynamodb", **kwargs)


def _s3(local: bool):
    kwargs = {"region_name": "us-east-1"}
    if local:
        kwargs["endpoint_url"] = os.environ.get("S3_ENDPOINT", "http://localhost:4566")
        kwargs["config"] = boto3.session.Config(signature_version="s3v4")
        os.environ.setdefault("AWS_ACCESS_KEY_ID", "test")
        os.environ.setdefault("AWS_SECRET_ACCESS_KEY", "test")
    return boto3.client("s3", **kwargs)


def _sign_model(model_bytes: bytes) -> bytes:
    """Sign model bytes with the Ed25519 private key."""
    from cryptography.hazmat.primitives.serialization import load_pem_private_key
    key_path = os.path.join(os.path.dirname(__file__), "..", "signing_key", "private.pem")
    if not os.path.exists(key_path):
        print("WARNING: signing_key/private.pem not found — using empty signature.")
        return b""
    with open(key_path, "rb") as f:
        private_key = load_pem_private_key(f.read(), password=None)
    return private_key.sign(model_bytes)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--model-id", required=True)
    parser.add_argument("--epoch-number", type=int, required=True)
    parser.add_argument("--threshold", type=int, default=2)
    parser.add_argument("--architecture-hash", default="arch-v1")
    parser.add_argument("--fedprox-mu", type=float, default=0.01)
    parser.add_argument("--epsilon", type=float, default=1.0)
    parser.add_argument("--delta", type=float, default=1e-5)
    parser.add_argument("--local", action="store_true")
    args = parser.parse_args()

    bucket = os.environ.get("BUCKET_NAME", "fl-ingestion-bucket")
    epoch_table_name = os.environ.get("EPOCH_TABLE", "FederatedEpochTable")

    # 1. Create a placeholder model (random 100-element float32 array → .npy)
    model_array = np.zeros(100, dtype=np.float32)
    buf = io.BytesIO()
    np.save(buf, model_array)
    model_bytes = buf.getvalue()

    model_hash = hashlib.sha256(model_bytes).hexdigest()
    model_version = f"v{args.epoch_number}"
    s3_key = f"models/{args.model_id}/{model_version}/model.npy"
    signature = _sign_model(model_bytes)

    # 2. Upload model to S3
    s3 = _s3(args.local)
    try:
        s3.create_bucket(Bucket=bucket)
    except Exception:
        pass  # bucket may already exist

    s3.put_object(Bucket=bucket, Key=s3_key, Body=model_bytes)
    print(f"Uploaded placeholder model to s3://{bucket}/{s3_key}")

    # 3. Write epoch record as PENDING
    epoch_id = f"EPOCH#{args.model_id}#{args.epoch_number}"
    ddb = _ddb(args.local)
    table = ddb.Table(epoch_table_name)
    table.put_item(Item={
        "epoch_id": epoch_id,
        "model_id": args.model_id,
        "epoch_number": args.epoch_number,
        "status": "PENDING",
        "model_version": model_version,
        "model_hash": model_hash,
        "model_s3_key": s3_key,
        "model_signature": base64.b64encode(signature).decode(),
        "architecture_hash": args.architecture_hash,
        "fedprox_mu": str(args.fedprox_mu),
        "privacy_epsilon": str(args.epsilon),
        "privacy_delta": str(args.delta),
        "secure_agg_threshold": args.threshold,
        "drift_alerts": "[]",
        "dataset_schema": "null",
        "created_at": datetime.now(timezone.utc).isoformat(),
    })

    print(f"Created epoch {epoch_id} (status=PENDING, threshold={args.threshold})")
    print(f"  model_hash={model_hash[:16]}...")
    print(f"\nNext: python scripts/activate_epoch.py --epoch-id \"{epoch_id}\" --local")


if __name__ == "__main__":
    main()
