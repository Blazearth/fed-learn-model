"""Auth integration tests (Task 9.3)."""
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


class TestAuth:
    def test_valid_org_accepted(self, aws):
        from epoch_query.app import handler
        resp = handler(make_event(org_id="org-a", query_params={"model_id": "fraud-v2"}), {})
        assert resp["statusCode"] == 200

    def test_unknown_org_rejected_403(self, aws):
        from epoch_query.app import handler
        resp = handler(make_event(org_id="org-unknown", query_params={"model_id": "fraud-v2"}), {})
        assert resp["statusCode"] == 403
        assert "not registered" in resp["body"]

    def test_suspended_org_rejected_403(self, aws):
        aws["ddb"].Table("OrgTable").put_item(
            Item={"org_id": "org-sus", "status": "SUSPENDED", "display_name": "Sus"}
        )
        from epoch_query.app import handler
        resp = handler(make_event(org_id="org-sus", query_params={"model_id": "fraud-v2"}), {})
        assert resp["statusCode"] == 403
        assert "suspended" in resp["body"]

    def test_duplicate_submission_returns_409(self, aws):
        from update_complete.app import handler
        body = {"epoch": 1, "model_id": "fraud-v2", "update_hash": "a" * 64}
        handler(make_event(org_id="org-a", body=body), {})
        resp = handler(make_event(org_id="org-a", body=body), {})
        assert resp["statusCode"] == 409

    def test_malformed_hash_returns_400(self, aws):
        from update_complete.app import handler
        body = {"epoch": 1, "model_id": "fraud-v2", "update_hash": "not-a-hash"}
        resp = handler(make_event(org_id="org-a", body=body), {})
        assert resp["statusCode"] == 400
