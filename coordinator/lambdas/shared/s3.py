"""S3 helpers — works with LocalStack (local) and real AWS."""
import os
import boto3


def _client():
    kwargs = {}
    endpoint = os.environ.get("S3_ENDPOINT")
    if endpoint:
        # LocalStack needs path-style addressing
        kwargs["endpoint_url"] = endpoint
        kwargs["config"] = boto3.session.Config(signature_version="s3v4")
    return boto3.client(
        "s3",
        region_name=os.environ.get("AWS_DEFAULT_REGION", "us-east-1"),
        **kwargs,
    )


def generate_presigned_get(bucket: str, key: str, expiry: int = 900) -> str:
    return _client().generate_presigned_url(
        "get_object",
        Params={"Bucket": bucket, "Key": key},
        ExpiresIn=expiry,
    )


def generate_presigned_put(bucket: str, key: str, expiry: int = 1800) -> str:
    return _client().generate_presigned_url(
        "put_object",
        Params={"Bucket": bucket, "Key": key},
        ExpiresIn=expiry,
    )


def object_exists(bucket: str, key: str) -> bool:
    try:
        _client().head_object(Bucket=bucket, Key=key)
        return True
    except _client().exceptions.ClientError:
        return False


def get_object_size(bucket: str, key: str) -> int | None:
    """Returns object size in bytes, or None if object does not exist."""
    try:
        resp = _client().head_object(Bucket=bucket, Key=key)
        return resp["ContentLength"]
    except Exception:
        return None


def compute_object_sha256(bucket: str, key: str) -> str | None:
    """Download object and return its SHA-256 hex digest, or None on error."""
    import hashlib
    try:
        resp = _client().get_object(Bucket=bucket, Key=key)
        data = resp["Body"].read()
        return hashlib.sha256(data).hexdigest()
    except Exception:
        return None
