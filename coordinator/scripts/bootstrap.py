#!/usr/bin/env python3
"""
Bootstrap script — creates DynamoDB tables + S3 bucket in LocalStack/DynamoDB Local,
generates signing key, seeds orgs and initial epoch.
Runs once on `docker compose up` via the setup service.
"""
import base64
import hashlib
import io
import json
import os
import sys
import time
from datetime import datetime, timezone

import boto3
import numpy as np
from botocore.exceptions import ClientError
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives.serialization import (
    Encoding, NoEncryption, PrivateFormat, PublicFormat,
)

DDB_ENDPOINT = os.environ.get("DYNAMODB_ENDPOINT", "http://dynamodb:8000")
S3_ENDPOINT  = os.environ.get("S3_ENDPOINT",  "http://localstack:4566")
REGION       = os.environ.get("AWS_DEFAULT_REGION", "us-east-1")
BUCKET       = os.environ.get("BUCKET_NAME", "fl-ingestion-bucket")

EPOCH_TABLE      = os.environ.get("EPOCH_TABLE",      "FederatedEpochTable")
SUBMISSION_TABLE = os.environ.get("SUBMISSION_TABLE", "FederatedSubmissionTable")
AUDIT_TABLE      = os.environ.get("AUDIT_TABLE",      "FederatedAuditTable")
ORG_TABLE        = os.environ.get("ORG_TABLE",         "FederatedOrgTable")


def ddb():
    return boto3.resource("dynamodb", region_name=REGION, endpoint_url=DDB_ENDPOINT)

def s3():
    return boto3.client("s3", region_name=REGION, endpoint_url=S3_ENDPOINT,
                        config=boto3.session.Config(signature_version="s3v4"))


def wait_for_services():
    import urllib.request, urllib.error
    for name, url in [("DynamoDB", DDB_ENDPOINT), ("LocalStack", S3_ENDPOINT)]:
        for attempt in range(30):
            try:
                urllib.request.urlopen(url, timeout=2)
                print(f"{name} ready.")
                break
            except Exception:
                print(f"Waiting for {name}... ({attempt+1}/30)")
                time.sleep(2)


def create_tables():
    db = ddb()

    tables = [
        dict(
            TableName=ORG_TABLE, BillingMode="PAY_PER_REQUEST",
            AttributeDefinitions=[{"AttributeName": "org_id", "AttributeType": "S"}],
            KeySchema=[{"AttributeName": "org_id", "KeyType": "HASH"}],
        ),
        dict(
            TableName=EPOCH_TABLE, BillingMode="PAY_PER_REQUEST",
            AttributeDefinitions=[
                {"AttributeName": "epoch_id",  "AttributeType": "S"},
                {"AttributeName": "model_id",  "AttributeType": "S"},
                {"AttributeName": "status",    "AttributeType": "S"},
            ],
            KeySchema=[{"AttributeName": "epoch_id", "KeyType": "HASH"}],
            GlobalSecondaryIndexes=[{
                "IndexName": "model_id-status-index",
                "KeySchema": [
                    {"AttributeName": "model_id", "KeyType": "HASH"},
                    {"AttributeName": "status",   "KeyType": "RANGE"},
                ],
                "Projection": {"ProjectionType": "ALL"},
            }],
        ),
        dict(
            TableName=SUBMISSION_TABLE, BillingMode="PAY_PER_REQUEST",
            AttributeDefinitions=[
                {"AttributeName": "submission_id", "AttributeType": "S"},
                {"AttributeName": "epoch_id",      "AttributeType": "S"},
                {"AttributeName": "org_id",        "AttributeType": "S"},
            ],
            KeySchema=[{"AttributeName": "submission_id", "KeyType": "HASH"}],
            GlobalSecondaryIndexes=[{
                "IndexName": "epoch_id-org_id-index",
                "KeySchema": [
                    {"AttributeName": "epoch_id", "KeyType": "HASH"},
                    {"AttributeName": "org_id",   "KeyType": "RANGE"},
                ],
                "Projection": {"ProjectionType": "ALL"},
            }],
            StreamSpecification={"StreamEnabled": True, "StreamViewType": "NEW_IMAGE"},
        ),
        dict(
            TableName=AUDIT_TABLE, BillingMode="PAY_PER_REQUEST",
            AttributeDefinitions=[
                {"AttributeName": "entry_id",   "AttributeType": "S"},
                {"AttributeName": "model_id",   "AttributeType": "S"},
                {"AttributeName": "created_at", "AttributeType": "S"},
            ],
            KeySchema=[{"AttributeName": "entry_id", "KeyType": "HASH"}],
            GlobalSecondaryIndexes=[{
                "IndexName": "model_id-created_at-index",
                "KeySchema": [
                    {"AttributeName": "model_id",   "KeyType": "HASH"},
                    {"AttributeName": "created_at", "KeyType": "RANGE"},
                ],
                "Projection": {"ProjectionType": "ALL"},
            }],
        ),
    ]

    for spec in tables:
        try:
            db.create_table(**spec)
            print(f"Created table: {spec['TableName']}")
        except ClientError as e:
            if "ResourceInUseException" in str(e):
                print(f"Table exists: {spec['TableName']}")
            else:
                raise


def create_bucket():
    try:
        s3().create_bucket(Bucket=BUCKET)
        print(f"Created bucket: {BUCKET}")
    except ClientError as e:
        if "BucketAlreadyOwnedByYou" in str(e) or "BucketAlreadyExists" in str(e):
            print(f"Bucket exists: {BUCKET}")
        else:
            raise


def generate_signing_key():
    key_dir = "/app/signing_key"
    priv_path = f"{key_dir}/private.pem"
    if os.path.exists(priv_path):
        print("Signing key already exists.")
        return

    os.makedirs(key_dir, exist_ok=True)
    priv = Ed25519PrivateKey.generate()
    priv_pem = priv.private_bytes(Encoding.PEM, PrivateFormat.PKCS8, NoEncryption())
    pub_pem  = priv.public_key().public_bytes(Encoding.PEM, PublicFormat.SubjectPublicKeyInfo)
    with open(priv_path, "wb") as f: f.write(priv_pem)
    with open(f"{key_dir}/public.pem", "wb") as f: f.write(pub_pem)
    print("Generated Ed25519 signing key pair.")


def seed_orgs():
    db = ddb()
    table = db.Table(ORG_TABLE)
    for org_id, name in [("org-hospital-a", "Hospital A"), ("org-hospital-b", "Hospital B")]:
        table.put_item(Item={
            "org_id": org_id, "display_name": name,
            "status": "ACTIVE", "public_key": "",
            "registered_at": datetime.now(timezone.utc).isoformat(),
        })
    print("Seeded 2 organizations.")


def seed_epoch():
    from cryptography.hazmat.primitives.serialization import load_pem_private_key

    model_array = np.zeros(100, dtype=np.float32)
    buf = io.BytesIO()
    np.save(buf, model_array)
    model_bytes = buf.getvalue()
    model_hash = hashlib.sha256(model_bytes).hexdigest()

    # Sign with generated key
    key_path = "/app/signing_key/private.pem"
    with open(key_path, "rb") as f:
        priv = load_pem_private_key(f.read(), password=None)
    signature = base64.b64encode(priv.sign(model_bytes)).decode()

    model_id = "fraud-detection-v2"
    s3_key = f"models/{model_id}/v1/model.npy"
    s3().put_object(Bucket=BUCKET, Key=s3_key, Body=model_bytes)

    db = ddb()
    epoch_id = f"EPOCH#{model_id}#1"
    db.Table(EPOCH_TABLE).put_item(Item={
        "epoch_id": epoch_id, "model_id": model_id,
        "epoch_number": 1, "status": "PENDING",
        "model_version": "v1", "model_hash": model_hash,
        "model_s3_key": s3_key, "model_signature": signature,
        "architecture_hash": "arch-v1",
        "fedprox_mu": "0.01", "privacy_epsilon": "1.0", "privacy_delta": "0.00001",
        "secure_agg_threshold": 2,
        "drift_alerts": "[]", "dataset_schema": "null",
        "created_at": datetime.now(timezone.utc).isoformat(),
    })

    # Activate with lock item
    try:
        db.Table(EPOCH_TABLE).put_item(
            Item={"epoch_id": f"MODEL#{model_id}#LOCK", "active_epoch_id": epoch_id},
            ConditionExpression="attribute_not_exists(epoch_id)",
        )
        db.Table(EPOCH_TABLE).update_item(
            Key={"epoch_id": epoch_id},
            UpdateExpression="SET #s = :active",
            ExpressionAttributeNames={"#s": "status"},
            ExpressionAttributeValues={":active": "ACTIVE"},
        )
        print(f"Seeded and activated epoch: {epoch_id}")
    except ClientError as e:
        if "ConditionalCheckFailedException" in str(e):
            print(f"Epoch already active for {model_id}.")
        else:
            raise


if __name__ == "__main__":
    wait_for_services()
    create_tables()
    create_bucket()
    generate_signing_key()
    seed_orgs()
    seed_epoch()
    print("\nBootstrap complete. Coordinator ready.")
