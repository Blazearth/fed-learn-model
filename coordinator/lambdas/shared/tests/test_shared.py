"""Unit tests for shared Lambda modules — auth, audit, response, dynamodb, s3."""
import hashlib
import json
import os
import sys

import pytest

# ── path setup ────────────────────────────────────────────────────────────────
# Allow importing 'shared' as a package from the lambdas/ directory
sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", ".."))

import boto3
from moto import mock_aws

# Force LOCAL_MODE and fake AWS creds before importing shared modules
os.environ.setdefault("LOCAL_MODE", "true")
os.environ.setdefault("AWS_DEFAULT_REGION", "us-east-1")
os.environ.setdefault("AWS_ACCESS_KEY_ID", "test")
os.environ.setdefault("AWS_SECRET_ACCESS_KEY", "test")
os.environ.setdefault("EPOCH_TABLE", "EpochTable")
os.environ.setdefault("SUBMISSION_TABLE", "SubmissionTable")
os.environ.setdefault("AUDIT_TABLE", "AuditTable")
os.environ.setdefault("ORG_TABLE", "OrgTable")
os.environ.setdefault("BUCKET_NAME", "test-bucket")

from shared.auth import AuthError, _extract_cn, get_authenticated_org
from shared.audit import GENESIS_HASH, write_audit_entry
from shared.response import bad_request, conflict, forbidden, not_found, ok


# ── Fixtures ──────────────────────────────────────────────────────────────────

@pytest.fixture()
def ddb():
    """Mocked DynamoDB with OrgTable and AuditTable pre-created."""
    with mock_aws():
        client = boto3.resource("dynamodb", region_name="us-east-1")

        # OrgTable
        client.create_table(
            TableName="OrgTable",
            BillingMode="PAY_PER_REQUEST",
            AttributeDefinitions=[{"AttributeName": "org_id", "AttributeType": "S"}],
            KeySchema=[{"AttributeName": "org_id", "KeyType": "HASH"}],
        )

        # AuditTable with GSI
        client.create_table(
            TableName="AuditTable",
            BillingMode="PAY_PER_REQUEST",
            AttributeDefinitions=[
                {"AttributeName": "entry_id", "AttributeType": "S"},
                {"AttributeName": "model_id", "AttributeType": "S"},
                {"AttributeName": "created_at", "AttributeType": "S"},
            ],
            KeySchema=[{"AttributeName": "entry_id", "KeyType": "HASH"}],
            GlobalSecondaryIndexes=[
                {
                    "IndexName": "model_id-created_at-index",
                    "KeySchema": [
                        {"AttributeName": "model_id", "KeyType": "HASH"},
                        {"AttributeName": "created_at", "KeyType": "RANGE"},
                    ],
                    "Projection": {"ProjectionType": "ALL"},
                }
            ],
        )

        yield client


# ── auth.py tests ─────────────────────────────────────────────────────────────

class TestExtractCn:
    def test_simple_cn(self):
        assert _extract_cn("CN=org-acme-bank") == "org-acme-bank"

    def test_cn_with_other_fields(self):
        assert _extract_cn("CN=org-hospital-a,O=Hospital A,C=US") == "org-hospital-a"

    def test_cn_with_spaces(self):
        assert _extract_cn("O=ACME, CN = org-acme-bank , C=US") == "org-acme-bank"

    def test_missing_cn_returns_empty(self):
        assert _extract_cn("O=ACME,C=US") == ""

    def test_empty_string(self):
        assert _extract_cn("") == ""


class TestGetAuthenticatedOrg:
    def test_valid_active_org(self, ddb):
        ddb.Table("OrgTable").put_item(
            Item={"org_id": "org-a", "status": "ACTIVE", "display_name": "Org A"}
        )
        event = {"headers": {"x-test-org-id": "org-a"}}
        assert get_authenticated_org(event) == "org-a"

    def test_missing_header_raises(self, ddb):
        with pytest.raises(AuthError) as exc:
            get_authenticated_org({"headers": {}})
        assert exc.value.response["statusCode"] == 403

    def test_unknown_org_raises(self, ddb):
        event = {"headers": {"x-test-org-id": "org-unknown"}}
        with pytest.raises(AuthError) as exc:
            get_authenticated_org(event)
        assert exc.value.response["statusCode"] == 403
        assert "not registered" in exc.value.response["body"]

    def test_suspended_org_raises(self, ddb):
        ddb.Table("OrgTable").put_item(
            Item={"org_id": "org-b", "status": "SUSPENDED", "display_name": "Org B"}
        )
        event = {"headers": {"x-test-org-id": "org-b"}}
        with pytest.raises(AuthError) as exc:
            get_authenticated_org(event)
        assert exc.value.response["statusCode"] == 403
        assert "not active" in exc.value.response["body"].lower()


# ── audit.py tests ────────────────────────────────────────────────────────────

class TestWriteAuditEntry:
    def test_first_entry_uses_genesis_hash(self, ddb):
        write_audit_entry("model-x", 1, "EPOCH_ACTIVATED", "SYSTEM", "{}")
        items = ddb.Table("AuditTable").scan()["Items"]
        assert len(items) == 1
        assert items[0]["previous_hash"] == GENESIS_HASH

    def test_entry_hash_is_correct(self, ddb):
        write_audit_entry("model-x", 1, "UPDATE_SUBMITTED", "org-a", '{"k":"v"}')
        item = ddb.Table("AuditTable").scan()["Items"][0]
        expected = hashlib.sha256(
            f"{item['entry_id']}UPDATE_SUBMITTEDorg-a{{\"k\":\"v\"}}{GENESIS_HASH}".encode()
        ).hexdigest()
        assert item["entry_hash"] == expected

    def test_second_entry_chains_to_first(self, ddb):
        write_audit_entry("model-y", 1, "EPOCH_ACTIVATED", "SYSTEM", "{}")
        first = ddb.Table("AuditTable").scan()["Items"][0]

        write_audit_entry("model-y", 1, "UPDATE_SUBMITTED", "org-a", "{}")
        items = sorted(
            ddb.Table("AuditTable").scan()["Items"],
            key=lambda x: x["created_at"],
        )
        assert items[1]["previous_hash"] == first["entry_hash"]

    def test_audit_failure_does_not_raise(self, monkeypatch, ddb):
        # Even if DynamoDB explodes, audit must not propagate the error
        from shared import audit as audit_mod
        monkeypatch.setattr(audit_mod, "put_item", lambda *a, **kw: (_ for _ in ()).throw(RuntimeError("boom")))
        # Should not raise
        write_audit_entry("model-z", 1, "TEST", "org-a", "{}")


# ── response.py tests ─────────────────────────────────────────────────────────

class TestResponse:
    def test_ok(self):
        r = ok({"status": "recorded"})
        assert r["statusCode"] == 200
        assert json.loads(r["body"]) == {"status": "recorded"}
        assert r["headers"]["Content-Type"] == "application/json"

    def test_bad_request(self):
        r = bad_request("invalid hash")
        assert r["statusCode"] == 400
        assert "invalid hash" in r["body"]

    def test_forbidden(self):
        r = forbidden("not allowed")
        assert r["statusCode"] == 403

    def test_not_found(self):
        r = not_found("no such epoch")
        assert r["statusCode"] == 404

    def test_conflict(self):
        r = conflict("already submitted")
        assert r["statusCode"] == 409


# ── dynamodb.py tests ─────────────────────────────────────────────────────────

class TestDynamoDB:
    def test_put_and_get(self, ddb):
        from shared.dynamodb import get_item, put_item
        put_item("ORG_TABLE", {"org_id": "org-test", "status": "ACTIVE"})
        item = get_item("ORG_TABLE", {"org_id": "org-test"})
        assert item["status"] == "ACTIVE"

    def test_get_missing_returns_none(self, ddb):
        from shared.dynamodb import get_item
        assert get_item("ORG_TABLE", {"org_id": "no-such-org"}) is None

    def test_conditional_put_success(self, ddb):
        from shared.dynamodb import put_item
        result = put_item(
            "ORG_TABLE",
            {"org_id": "org-new", "status": "ACTIVE"},
            condition="attribute_not_exists(org_id)",
        )
        assert result is True

    def test_conditional_put_failure(self, ddb):
        from shared.dynamodb import put_item
        put_item("ORG_TABLE", {"org_id": "org-dup", "status": "ACTIVE"})
        result = put_item(
            "ORG_TABLE",
            {"org_id": "org-dup", "status": "ACTIVE"},
            condition="attribute_not_exists(org_id)",
        )
        assert result is False
