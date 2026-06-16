"""Shared fixtures for Lambda endpoint tests."""
import os, sys
import boto3
import pytest
from moto import mock_aws

os.environ.setdefault("LOCAL_MODE", "true")
os.environ.setdefault("AWS_DEFAULT_REGION", "us-east-1")
os.environ.setdefault("AWS_ACCESS_KEY_ID", "test")
os.environ.setdefault("AWS_SECRET_ACCESS_KEY", "test")
os.environ.setdefault("EPOCH_TABLE", "EpochTable")
os.environ.setdefault("SUBMISSION_TABLE", "SubmissionTable")
os.environ.setdefault("AUDIT_TABLE", "AuditTable")
os.environ.setdefault("ORG_TABLE", "OrgTable")
os.environ.setdefault("BUCKET_NAME", "test-bucket")

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))


@pytest.fixture()
def aws_tables():
    with mock_aws():
        ddb = boto3.resource("dynamodb", region_name="us-east-1")

        ddb.create_table(
            TableName="OrgTable", BillingMode="PAY_PER_REQUEST",
            AttributeDefinitions=[{"AttributeName": "org_id", "AttributeType": "S"}],
            KeySchema=[{"AttributeName": "org_id", "KeyType": "HASH"}],
        )
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

        # Seed active org
        ddb.Table("OrgTable").put_item(Item={
            "org_id": "org-a", "status": "ACTIVE",
            "display_name": "Org A", "public_key": "pubkey-a",
        })
        ddb.Table("OrgTable").put_item(Item={
            "org_id": "org-b", "status": "ACTIVE",
            "display_name": "Org B", "public_key": "pubkey-b",
        })

        # Seed active epoch
        ddb.Table("EpochTable").put_item(Item={
            "epoch_id": "EPOCH#fraud-v2#1",
            "model_id": "fraud-v2",
            "epoch_number": 1,
            "status": "ACTIVE",
            "model_version": "v1",
            "model_hash": "a" * 64,
            "model_signature": "sig123",
            "architecture_hash": "arch-abc",
            "fedprox_mu": "0.01",
            "privacy_epsilon": "1.0",
            "privacy_delta": "0.00001",
            "secure_agg_threshold": 2,
            "drift_alerts": "[]",
            "dataset_schema": "null",
        })

        yield ddb


def make_event(org_id="org-a", query_params=None, body=None):
    return {
        "headers": {"x-test-org-id": org_id},
        "queryStringParameters": query_params or {},
        "body": __import__("json").dumps(body) if body else None,
    }
