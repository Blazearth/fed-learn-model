"""
Integration test: complete federated training round simulation (Task 9.2).
Tests the full workflow: poll → download URL → upload URL → complete × N → trigger.
"""
import hashlib
import json
import sys
import os
import pytest

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "lambdas"))


def make_event(org_id="org-a", query_params=None, body=None):
    return {
        "headers": {"x-test-org-id": org_id},
        "queryStringParameters": query_params or {},
        "body": json.dumps(body) if body else None,
    }


class TestFullRound:
    def test_step1_poll_returns_epoch_metadata(self, aws):
        from epoch_query.app import handler
        event = make_event(query_params={"model_id": "fraud-v2"})
        resp = handler(event, {})
        assert resp["statusCode"] == 200
        body = json.loads(resp["body"])
        # All fields the daemon requires
        for field in ["epoch_number", "model_id", "model_version", "model_hash",
                      "model_signature", "architecture_hash", "fedprox_mu",
                      "privacy_epsilon", "privacy_delta",
                      "secure_agg_participants", "secure_agg_threshold",
                      "drift_alerts", "dataset_schema"]:
            assert field in body, f"Missing field: {field}"
        assert body["epoch_number"] == 1
        assert len(body["model_hash"]) == 64

    def test_step2_model_download_url_returned(self, aws):
        from model_url.app import handler
        event = make_event(body={"model_id": "fraud-v2", "model_version": "v1"})
        resp = handler(event, {})
        assert resp["statusCode"] == 200
        assert "url" in json.loads(resp["body"])

    def test_step3_upload_url_returned_with_deterministic_key(self, aws):
        from update_url.app import handler
        event = make_event(org_id="org-a", body={"model_id": "fraud-v2", "epoch_number": 1})
        resp = handler(event, {})
        assert resp["statusCode"] == 200
        url = json.loads(resp["body"])["url"]
        assert "updates/fraud-v2/1/org-a/update.bin" in url

    def test_step4_two_orgs_submit_completions(self, aws):
        from update_complete.app import handler
        update_hash = "a" * 64

        # org-a submits
        resp_a = handler(make_event(org_id="org-a", body={
            "epoch": 1, "model_id": "fraud-v2", "update_hash": update_hash,
        }), {})
        assert resp_a["statusCode"] == 200

        # org-b submits
        resp_b = handler(make_event(org_id="org-b", body={
            "epoch": 1, "model_id": "fraud-v2", "update_hash": update_hash,
        }), {})
        assert resp_b["statusCode"] == 200

        # Both submissions are in DynamoDB
        submissions = aws["ddb"].Table("SubmissionTable").scan()["Items"]
        assert len(submissions) == 2
        orgs = {s["org_id"] for s in submissions}
        assert orgs == {"org-a", "org-b"}

    def test_step5_audit_entries_written_for_both_orgs(self, aws):
        from update_complete.app import handler
        from shared.dynamodb import query_gsi

        update_hash = "b" * 64
        for org in ["org-a", "org-b"]:
            handler(make_event(org_id=org, body={
                "epoch": 1, "model_id": "fraud-v2", "update_hash": update_hash,
            }), {})

        entries = query_gsi(
            "AUDIT_TABLE", "model_id-created_at-index",
            pk_name="model_id", pk_value="fraud-v2",
        )
        submitted_entries = [e for e in entries if e["event_type"] == "UPDATE_SUBMITTED"]
        assert len(submitted_entries) == 2

    def test_step6_threshold_reached_triggers_aggregation(self, aws, monkeypatch):
        """When N submissions = threshold, aggregation trigger fires."""
        from update_complete.app import handler as complete_handler
        from aggregation_trigger.app import handler as trigger_handler

        agg_called = []
        monkeypatch.setattr(
            "aggregation_trigger.app._run_local_aggregation",
            lambda *a, **kw: agg_called.append(True),
        )

        update_hash = "c" * 64
        # Submit for both orgs
        for org in ["org-a", "org-b"]:
            complete_handler(make_event(org_id=org, body={
                "epoch": 1, "model_id": "fraud-v2", "update_hash": update_hash,
            }), {})

        # Simulate DynamoDB Stream event for the 2nd submission
        trigger_event = {
            "Records": [{
                "eventName": "INSERT",
                "dynamodb": {"NewImage": {
                    "epoch_id": {"S": "EPOCH#fraud-v2#1"},
                    "model_id": {"S": "fraud-v2"},
                }},
            }]
        }
        trigger_handler(trigger_event, {})

        # Epoch should now be AGGREGATING
        epoch = aws["ddb"].Table("EpochTable").get_item(
            Key={"epoch_id": "EPOCH#fraud-v2#1"}
        )["Item"]
        assert epoch["status"] == "AGGREGATING"
        assert agg_called, "Aggregation must be triggered when threshold met"
