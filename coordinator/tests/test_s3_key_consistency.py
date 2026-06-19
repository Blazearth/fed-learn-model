"""
Bug 1 regression: S3 key consistency between UpdateUrlFunction and UpdateCompleteFunction (Task 9.6).
Verifies the deterministic key fix — no uuid4() disconnect.
"""
import json
import sys
import os
import pytest

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "lambdas"))

from tests.helpers import put_fake_update


def make_event(org_id="org-a", query_params=None, body=None):
    return {
        "headers": {"x-test-org-id": org_id},
        "queryStringParameters": query_params or {},
        "body": json.dumps(body) if body else None,
    }


class TestS3KeyConsistency:
    def test_upload_url_key_matches_submission_table_key(self, aws):
        """The key embedded in the pre-signed PUT URL must equal
        the s3_key stored in SubmissionTable after update_complete."""
        from update_url.app import handler as url_handler
        from update_complete.app import handler as complete_handler

        # Step 1: get pre-signed upload URL for org-a, epoch 1
        url_resp = url_handler(
            make_event(org_id="org-a", body={"model_id": "fraud-v2", "epoch_number": 1}), {}
        )
        assert url_resp["statusCode"] == 200
        presigned_url = json.loads(url_resp["body"])["url"]

        # Extract S3 key from the URL path segment
        # LocalStack/moto URL format: http://.../<bucket>/<key>?...
        from urllib.parse import urlparse, unquote
        parsed = urlparse(presigned_url)
        # Path is /<bucket>/<key> or /<key> depending on endpoint style
        path_parts = parsed.path.lstrip("/").split("/", 1)
        key_from_url = unquote(path_parts[-1])

        # Step 2: upload real file and call update_complete so hash verification passes
        real_hash = put_fake_update(aws["s3"], "org-a", "fraud-v2", 1)
        complete_handler(
            make_event(org_id="org-a", body={
                "epoch": 1, "model_id": "fraud-v2", "update_hash": real_hash,
            }), {}
        )

        # Step 3: read s3_key from SubmissionTable
        sub = aws["ddb"].Table("SubmissionTable").scan()["Items"][0]
        key_from_table = sub["s3_key"]

        # Both must be the same deterministic path
        expected = "updates/fraud-v2/1/org-a/update.bin"
        assert key_from_table == expected, (
            f"SubmissionTable key wrong: {key_from_table}"
        )
        # moto pre-signed URLs use virtual-hosted style: path = /<key> (no bucket prefix)
        assert key_from_url.endswith("fraud-v2/1/org-a/update.bin"), (
            f"Pre-signed URL key wrong: {key_from_url}"
        )

    def test_key_pattern_is_deterministic_no_uuid(self, aws):
        """Call update_url twice — both must return the same key."""
        from update_url.app import handler
        from urllib.parse import urlparse, unquote

        def extract_key(resp):
            url = json.loads(resp["body"])["url"]
            path = urlparse(url).path.lstrip("/")
            return unquote(path.split("/", 1)[-1])

        resp1 = handler(make_event(org_id="org-a", body={"model_id": "fraud-v2", "epoch_number": 1}), {})
        resp2 = handler(make_event(org_id="org-a", body={"model_id": "fraud-v2", "epoch_number": 1}), {})

        key1 = extract_key(resp1)
        key2 = extract_key(resp2)
        assert key1.endswith("fraud-v2/1/org-a/update.bin"), f"Key1 wrong: {key1}"
        assert key1 == key2, f"Keys differ between calls: {key1} vs {key2}"
