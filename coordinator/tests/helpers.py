"""Shared test helpers — not a conftest so they can be imported directly."""
import hashlib
import io

import boto3
import numpy as np


def put_fake_update(
    s3_client,
    org_id: str,
    model_id: str,
    epoch_number: int,
    bucket: str = "test-bucket",
) -> str:
    """Upload a fake NPY update to the mocked S3 bucket.

    Returns the SHA-256 hex of the uploaded bytes so the caller can pass
    update_hash=<return value> to update_complete and pass hash verification.
    Uses a 1000-element array to stay above the minimum file size check.
    """
    buf = io.BytesIO()
    np.save(buf, np.random.rand(1000).astype(np.float32))
    data = buf.getvalue()
    key = f"updates/{model_id}/{epoch_number}/{org_id}/update.bin"
    s3_client.put_object(Bucket=bucket, Key=key, Body=data)
    return hashlib.sha256(data).hexdigest()
