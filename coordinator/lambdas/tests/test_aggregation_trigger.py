"""Unit tests for aggregation trigger Lambda (Task 5.2)."""
import json
import os
import sys
import pytest
from moto import mock_aws
import boto3

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))


def _make_stream_event(epoch_id: str, model_id: str) -> dict:
    return {
        "Records": [{
            "eventName": "INSERT",
            "dynamodb": {
                "NewImage": {
                    "epoch_id": {"S": epoch_id},
                    "model_id": {"S": model_id},
                    "org_id": {"S": "org-a"},
                    "submission_id": {"S": "SUB#001"},
                }
            }
        }]
    }


@pytest.fixture()
def aws_tables():
    with mock_aws():
        ddb = boto3.resource("dynamodb", region_name="us-east-1")

        ddb.create_table(
            TableName="EpochTable", BillingMode="PAY_PER_REQUEST",
            AttributeDefinitions=[
                {"AttributeName": "epoch_id", "AttributeType": "S"},
            ],
            KeySchema=[{"AttributeName": "epoch_id", "KeyType": "HASH"}],
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

        yield ddb


def _seed_epoch(ddb, epoch_id, threshold=2, status="ACTIVE"):
    ddb.Table("EpochTable").put_item(Item={
        "epoch_id": epoch_id,
        "model_id": "fraud-v2",
        "epoch_number": 1,
        "status": status,
        "secure_agg_threshold": threshold,
    })


def _seed_submission(ddb, epoch_id, org_id):
    ddb.Table("SubmissionTable").put_item(Item={
        "submission_id": f"SUB#{org_id}",
        "epoch_id": epoch_id,
        "org_id": org_id,
        "update_hash": "a" * 64,
    })


class TestAggregationTrigger:
    def test_count_below_threshold_does_not_trigger(self, aws_tables, monkeypatch):
        _seed_epoch(aws_tables, "EPOCH#fraud-v2#1", threshold=2)
        _seed_submission(aws_tables, "EPOCH#fraud-v2#1", "org-a")  # only 1 of 2

        launched = []
        monkeypatch.setattr(
            "aggregation_trigger.app._launch_fargate_task",
            lambda *a, **kw: launched.append(True)
        )
        monkeypatch.setattr(
            "aggregation_trigger.app._run_local_aggregation",
            lambda *a, **kw: launched.append(True)
        )

        from aggregation_trigger.app import handler
        handler(_make_stream_event("EPOCH#fraud-v2#1", "fraud-v2"), {})
        assert launched == [], "Should not trigger when below threshold"

    def test_count_at_threshold_sets_status_aggregating(self, aws_tables, monkeypatch):
        _seed_epoch(aws_tables, "EPOCH#fraud-v2#2", threshold=2)
        _seed_submission(aws_tables, "EPOCH#fraud-v2#2", "org-a")
        _seed_submission(aws_tables, "EPOCH#fraud-v2#2", "org-b")

        monkeypatch.setattr(
            "aggregation_trigger.app._run_local_aggregation",
            lambda *a, **kw: None
        )
        monkeypatch.setattr(
            "aggregation_trigger.app._launch_fargate_task",
            lambda *a, **kw: None
        )

        from aggregation_trigger.app import handler
        handler(_make_stream_event("EPOCH#fraud-v2#2", "fraud-v2"), {})

        epoch = aws_tables.Table("EpochTable").get_item(
            Key={"epoch_id": "EPOCH#fraud-v2#2"}
        )["Item"]
        assert epoch["status"] == "AGGREGATING"

    def test_already_aggregating_does_not_launch_again(self, aws_tables, monkeypatch):
        _seed_epoch(aws_tables, "EPOCH#fraud-v2#3", threshold=1, status="AGGREGATING")
        _seed_submission(aws_tables, "EPOCH#fraud-v2#3", "org-a")

        launched = []
        monkeypatch.setattr(
            "aggregation_trigger.app._run_local_aggregation",
            lambda *a, **kw: launched.append(True)
        )

        from aggregation_trigger.app import handler
        handler(_make_stream_event("EPOCH#fraud-v2#3", "fraud-v2"), {})
        assert launched == [], "Must not launch duplicate task — idempotency"

    def test_non_insert_event_is_skipped(self, aws_tables, monkeypatch):
        _seed_epoch(aws_tables, "EPOCH#fraud-v2#4", threshold=1)

        launched = []
        monkeypatch.setattr(
            "aggregation_trigger.app._run_local_aggregation",
            lambda *a, **kw: launched.append(True)
        )

        from aggregation_trigger.app import handler
        event = {"Records": [{"eventName": "MODIFY", "dynamodb": {"NewImage": {}}}]}
        handler(event, {})
        assert launched == [], "MODIFY events must be ignored"
