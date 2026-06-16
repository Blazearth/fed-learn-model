#!/usr/bin/env python3
"""
Activate a PENDING epoch — transitions it to ACTIVE using atomic Lock Item.
Enforces single-active-epoch invariant (Bug 2 fix).

Usage:
  python scripts/activate_epoch.py --epoch-id "EPOCH#fraud-detection-v2#1" --local
"""
import argparse
import os
from datetime import datetime, timezone

import boto3
from botocore.exceptions import ClientError


def _ddb(local: bool):
    kwargs = {"region_name": "us-east-1"}
    if local:
        kwargs["endpoint_url"] = os.environ.get("DYNAMODB_ENDPOINT", "http://localhost:8000")
        os.environ.setdefault("AWS_ACCESS_KEY_ID", "test")
        os.environ.setdefault("AWS_SECRET_ACCESS_KEY", "test")
    return boto3.resource("dynamodb", **kwargs)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--epoch-id", required=True,
                        help='e.g. "EPOCH#fraud-detection-v2#1"')
    parser.add_argument("--local", action="store_true")
    args = parser.parse_args()

    epoch_table_name = os.environ.get("EPOCH_TABLE", "FederatedEpochTable")
    ddb = _ddb(args.local)
    table = ddb.Table(epoch_table_name)

    # 1. Fetch epoch
    resp = table.get_item(Key={"epoch_id": args.epoch_id})
    epoch = resp.get("Item")
    if not epoch:
        print(f"ERROR: Epoch '{args.epoch_id}' not found.")
        return 1

    if epoch["status"] != "PENDING":
        print(f"ERROR: Epoch is '{epoch['status']}', must be PENDING to activate.")
        return 1

    model_id = epoch["model_id"]
    lock_key = f"MODEL#{model_id}#LOCK"

    # 2. Write Lock Item atomically — prevents TOCTOU race condition (Bug 2 fix)
    now = datetime.now(timezone.utc).isoformat()
    try:
        table.put_item(
            Item={
                "epoch_id": lock_key,
                "active_epoch_id": args.epoch_id,
                "activated_at": now,
            },
            ConditionExpression="attribute_not_exists(epoch_id)",
        )
    except ClientError as e:
        if e.response["Error"]["Code"] == "ConditionalCheckFailedException":
            print(f"ERROR: An active epoch already exists for model '{model_id}'.")
            print("       Complete or fail the current epoch before activating a new one.")
            return 1
        raise

    # 3. Set epoch status to ACTIVE
    table.update_item(
        Key={"epoch_id": args.epoch_id},
        UpdateExpression="SET #s = :active, activated_at = :now",
        ExpressionAttributeNames={"#s": "status"},
        ExpressionAttributeValues={":active": "ACTIVE", ":now": now},
    )

    print(f"Epoch '{args.epoch_id}' is now ACTIVE.")
    print(f"  Lock item written: {lock_key}")
    print(f"  Daemons polling for model_id='{model_id}' will now receive EpochMetadata.")


if __name__ == "__main__":
    raise SystemExit(main())
