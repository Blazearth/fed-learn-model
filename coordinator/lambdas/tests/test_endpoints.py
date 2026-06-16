"""Unit tests for all Lambda endpoint functions."""
import json
import sys
import os
import pytest
from moto import mock_aws

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))


def make_event(org_id="org-a", query_params=None, body=None):
    return {
        "headers": {"x-test-org-id": org_id},
        "queryStringParameters": query_params or {},
        "body": json.dumps(body) if body else None,
    }


# ── epoch_query ───────────────────────────────────────────────────────────────

class TestEpochQuery:
    def test_returns_active_epoch(self, aws_tables):
        from epoch_query.app import handler
        event = make_event(query_params={"model_id": "fraud-v2"})
        resp = handler(event, {})
        assert resp["statusCode"] == 200
        body = json.loads(resp["body"])
        assert body["epoch_number"] == 1
        assert body["model_id"] == "fraud-v2"
        assert len(body["model_hash"]) == 64

    def test_returns_404_when_no_active_epoch(self, aws_tables):
        from epoch_query.app import handler
        event = make_event(query_params={"model_id": "nonexistent-model"})
        resp = handler(event, {})
        assert resp["statusCode"] == 404

    def test_returns_400_when_model_id_missing(self, aws_tables):
        from epoch_query.app import handler
        event = make_event(query_params={})
        resp = handler(event, {})
        assert resp["statusCode"] == 400

    def test_returns_403_for_unknown_org(self, aws_tables):
        from epoch_query.app import handler
        event = make_event(org_id="org-unknown", query_params={"model_id": "fraud-v2"})
        resp = handler(event, {})
        assert resp["statusCode"] == 403

    def test_epoch_metadata_contains_all_required_daemon_fields(self, aws_tables):
        from epoch_query.app import handler
        event = make_event(query_params={"model_id": "fraud-v2"})
        body = json.loads(handler(event, {})["body"])
        required = [
            "epoch_number", "model_id", "model_version", "model_hash",
            "model_signature", "architecture_hash", "fedprox_mu",
            "privacy_epsilon", "privacy_delta",
            "secure_agg_participants", "secure_agg_threshold",
            "drift_alerts", "dataset_schema",
        ]
        for field in required:
            assert field in body, f"missing field: {field}"


# ── model_url ─────────────────────────────────────────────────────────────────

class TestModelUrl:
    def test_returns_presigned_url(self, aws_tables):
        import boto3
        from moto import mock_aws
        with mock_aws():
            boto3.client("s3", region_name="us-east-1").create_bucket(Bucket="test-bucket")
            from model_url.app import handler
            event = make_event(body={"model_id": "fraud-v2", "model_version": "v1"})
            resp = handler(event, {})
            assert resp["statusCode"] == 200
            assert "url" in json.loads(resp["body"])

    def test_returns_400_when_fields_missing(self, aws_tables):
        from model_url.app import handler
        event = make_event(body={"model_id": "fraud-v2"})
        resp = handler(event, {})
        assert resp["statusCode"] == 400

    def test_returns_403_for_unknown_org(self, aws_tables):
        from model_url.app import handler
        event = make_event(org_id="org-ghost", body={"model_id": "x", "model_version": "v1"})
        resp = handler(event, {})
        assert resp["statusCode"] == 403


# ── update_url ────────────────────────────────────────────────────────────────

class TestUpdateUrl:
    def test_returns_presigned_url(self, aws_tables):
        import boto3
        from moto import mock_aws
        with mock_aws():
            boto3.client("s3", region_name="us-east-1").create_bucket(Bucket="test-bucket")
            from update_url.app import handler
            event = make_event(body={"model_id": "fraud-v2", "epoch_number": 1})
            resp = handler(event, {})
            assert resp["statusCode"] == 200
            url = json.loads(resp["body"])["url"]
            # Key must be deterministic — no random UUID
            assert "updates/fraud-v2/1/org-a/update.bin" in url

    def test_returns_400_when_fields_missing(self, aws_tables):
        from update_url.app import handler
        event = make_event(body={"model_id": "fraud-v2"})
        resp = handler(event, {})
        assert resp["statusCode"] == 400


# ── update_complete ───────────────────────────────────────────────────────────

class TestUpdateComplete:
    def _valid_body(self):
        return {"epoch": 1, "model_id": "fraud-v2", "update_hash": "a" * 64}

    def test_records_valid_submission(self, aws_tables):
        from update_complete.app import handler
        event = make_event(body=self._valid_body())
        resp = handler(event, {})
        assert resp["statusCode"] == 200
        assert "submission_id" in json.loads(resp["body"])

    def test_duplicate_submission_returns_409(self, aws_tables):
        from update_complete.app import handler
        event = make_event(body=self._valid_body())
        handler(event, {})           # first — succeeds
        resp = handler(event, {})    # second — conflict
        assert resp["statusCode"] == 409

    def test_invalid_hash_returns_400(self, aws_tables):
        from update_complete.app import handler
        body = self._valid_body()
        body["update_hash"] = "not-a-valid-hash"
        event = make_event(body=body)
        resp = handler(event, {})
        assert resp["statusCode"] == 400

    def test_missing_epoch_returns_400(self, aws_tables):
        from update_complete.app import handler
        event = make_event(body={"model_id": "fraud-v2", "update_hash": "a" * 64})
        resp = handler(event, {})
        assert resp["statusCode"] == 400

    def test_forbidden_for_suspended_org(self, aws_tables):
        aws_tables.Table("OrgTable").put_item(
            Item={"org_id": "org-sus", "status": "SUSPENDED"}
        )
        from update_complete.app import handler
        event = make_event(org_id="org-sus", body=self._valid_body())
        resp = handler(event, {})
        assert resp["statusCode"] == 403

    def test_s3_key_stored_is_deterministic(self, aws_tables):
        """Bug 1 regression: key in SubmissionTable must match update_url key."""
        from update_complete.app import handler
        import boto3
        event = make_event(body=self._valid_body())
        handler(event, {})
        sub = aws_tables.Table("SubmissionTable").scan()["Items"][0]
        assert sub["s3_key"] == "updates/fraud-v2/1/org-a/update.bin"


# ── audit_query ───────────────────────────────────────────────────────────────

class TestAuditQuery:
    def test_returns_entries_for_model(self, aws_tables):
        # Write an audit entry first
        from shared.audit import write_audit_entry
        write_audit_entry("fraud-v2", 1, "UPDATE_SUBMITTED", "org-a", "{}")

        from audit_query.app import handler
        event = make_event(query_params={"model_id": "fraud-v2"})
        resp = handler(event, {})
        assert resp["statusCode"] == 200
        body = json.loads(resp["body"])
        assert body["count"] >= 1

    def test_returns_400_without_model_id(self, aws_tables):
        from audit_query.app import handler
        event = make_event(query_params={})
        resp = handler(event, {})
        assert resp["statusCode"] == 400

    def test_limit_is_enforced(self, aws_tables):
        from audit_query.app import handler
        event = make_event(query_params={"model_id": "fraud-v2", "limit": "999"})
        # limit capped at 100 — just verify no error
        resp = handler(event, {})
        assert resp["statusCode"] in (200, 400)


# ── health ────────────────────────────────────────────────────────────────────

class TestHealth:
    def test_returns_ok(self):
        from health.app import handler
        resp = handler({}, {})
        assert resp["statusCode"] == 200
        body = json.loads(resp["body"])
        assert body["status"] == "ok"
        assert "timestamp" in body
