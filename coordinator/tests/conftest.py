"""Shared fixtures for integration tests."""
import base64
import io
import os
import sys

import boto3
import numpy as np
import pytest
from moto import mock_aws

# Env must be set before any module import
os.environ["LOCAL_MODE"] = "true"
os.environ["AWS_DEFAULT_REGION"] = "us-east-1"
os.environ["AWS_ACCESS_KEY_ID"] = "test"
os.environ["AWS_SECRET_ACCESS_KEY"] = "test"
os.environ["EPOCH_TABLE"] = "EpochTable"
os.environ["SUBMISSION_TABLE"] = "SubmissionTable"
os.environ["AUDIT_TABLE"] = "AuditTable"
os.environ["ORG_TABLE"] = "OrgTable"
os.environ["BUCKET_NAME"] = "test-bucket"

# Add lambdas/ and aggregation/ to path
sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "lambdas"))
sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "aggregation"))

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives.serialization import (
    Encoding, NoEncryption, PrivateFormat, PublicFormat,
)


def make_keypair():
    priv = Ed25519PrivateKey.generate()
    priv_pem = priv.private_bytes(Encoding.PEM, PrivateFormat.PKCS8, NoEncryption())
    pub_pem = priv.public_key().public_bytes(Encoding.PEM, PublicFormat.SubjectPublicKeyInfo)
    return priv, priv_pem, pub_pem


def make_event(org_id="org-a", query_params=None, body=None):
    import json
    return {
        "headers": {"x-test-org-id": org_id},
        "queryStringParameters": query_params or {},
        "body": json.dumps(body) if body else None,
    }


def put_fake_update(s3_client, org_id: str, model_id: str, epoch_number: int) -> str:
    """Upload a fake NPY update to mocked S3. Returns the SHA-256 hex of the data."""
    from tests.helpers import put_fake_update as _helper
    return _helper(s3_client, org_id, model_id, epoch_number)


def _model_bytes():
    buf = io.BytesIO()
    np.save(buf, np.zeros(100, dtype=np.float32))
    return buf.getvalue()


@pytest.fixture()
def aws():
    """Full mocked AWS environment with all tables, bucket, and seeded data."""
    with mock_aws():
        ddb = boto3.resource("dynamodb", region_name="us-east-1")
        s3 = boto3.client("s3", region_name="us-east-1")

        # S3 bucket
        s3.create_bucket(Bucket="test-bucket")

        # OrgTable
        ddb.create_table(
            TableName="OrgTable", BillingMode="PAY_PER_REQUEST",
            AttributeDefinitions=[{"AttributeName": "org_id", "AttributeType": "S"}],
            KeySchema=[{"AttributeName": "org_id", "KeyType": "HASH"}],
        )
        # EpochTable
        ddb.create_table(
            TableName="EpochTable", BillingMode="PAY_PER_REQUEST",
            AttributeDefinitions=[
                {"AttributeName": "epoch_id", "AttributeType": "S"},
                {"AttributeName": "model_id", "AttributeType": "S"},
                {"AttributeName": "status", "AttributeType": "S"},
            ],
            KeySchema=[{"AttributeName": "epoch_id", "KeyType": "HASH"}],
            GlobalSecondaryIndexes=[{
                "IndexName": "model_id-status-index",
                "KeySchema": [
                    {"AttributeName": "model_id", "KeyType": "HASH"},
                    {"AttributeName": "status", "KeyType": "RANGE"},
                ],
                "Projection": {"ProjectionType": "ALL"},
            }],
        )
        # SubmissionTable
        ddb.create_table(
            TableName="SubmissionTable", BillingMode="PAY_PER_REQUEST",
            AttributeDefinitions=[
                {"AttributeName": "submission_id", "AttributeType": "S"},
                {"AttributeName": "epoch_id", "AttributeType": "S"},
                {"AttributeName": "org_id", "AttributeType": "S"},
            ],
            KeySchema=[{"AttributeName": "submission_id", "KeyType": "HASH"}],
            GlobalSecondaryIndexes=[{
                "IndexName": "epoch_id-org_id-index",
                "KeySchema": [
                    {"AttributeName": "epoch_id", "KeyType": "HASH"},
                    {"AttributeName": "org_id", "KeyType": "RANGE"},
                ],
                "Projection": {"ProjectionType": "ALL"},
            }],
            StreamSpecification={"StreamEnabled": True, "StreamViewType": "NEW_IMAGE"},
        )
        # AuditTable
        ddb.create_table(
            TableName="AuditTable", BillingMode="PAY_PER_REQUEST",
            AttributeDefinitions=[
                {"AttributeName": "entry_id", "AttributeType": "S"},
                {"AttributeName": "model_id", "AttributeType": "S"},
                {"AttributeName": "created_at", "AttributeType": "S"},
            ],
            KeySchema=[{"AttributeName": "entry_id", "KeyType": "HASH"}],
            GlobalSecondaryIndexes=[{
                "IndexName": "model_id-created_at-index",
                "KeySchema": [
                    {"AttributeName": "model_id", "KeyType": "HASH"},
                    {"AttributeName": "created_at", "KeyType": "RANGE"},
                ],
                "Projection": {"ProjectionType": "ALL"},
            }],
        )

        # Seed orgs
        for org in ["org-a", "org-b"]:
            ddb.Table("OrgTable").put_item(Item={
                "org_id": org, "status": "ACTIVE",
                "display_name": org, "public_key": "pk",
            })

        # Seed model in S3
        model_data = _model_bytes()
        import hashlib
        model_hash = hashlib.sha256(model_data).hexdigest()
        s3.put_object(Bucket="test-bucket", Key="models/fraud-v2/v1/model.npy", Body=model_data)

        # Seed ACTIVE epoch
        priv, priv_pem, pub_pem = make_keypair()
        sig = base64.b64encode(priv.sign(model_data)).decode()
        ddb.Table("EpochTable").put_item(Item={
            "epoch_id": "EPOCH#fraud-v2#1",
            "model_id": "fraud-v2",
            "epoch_number": 1,
            "status": "ACTIVE",
            "model_version": "v1",
            "model_hash": model_hash,
            "model_s3_key": "models/fraud-v2/v1/model.npy",
            "model_signature": sig,
            "architecture_hash": "arch-v1",
            "fedprox_mu": "0.01",
            "privacy_epsilon": "1.0",
            "privacy_delta": "0.00001",
            "secure_agg_threshold": 2,
            "drift_alerts": "[]",
            "dataset_schema": "null",
        })

        # Store private key in a temp dir so aggregator can find it
        import tempfile, pathlib
        key_dir = pathlib.Path(os.path.dirname(__file__), "..", "signing_key")
        key_dir.mkdir(exist_ok=True)
        (key_dir / "private.pem").write_bytes(priv_pem)
        (key_dir / "public.pem").write_bytes(pub_pem)

        yield {"ddb": ddb, "s3": s3, "model_hash": model_hash, "priv_pem": priv_pem}
