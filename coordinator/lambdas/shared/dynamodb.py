"""DynamoDB helpers — works with both LocalStack (local) and real AWS."""
import os
import boto3
from boto3.dynamodb.conditions import Key


def _client():
    kwargs = {}
    endpoint = os.environ.get("DYNAMODB_ENDPOINT")
    if endpoint:
        kwargs["endpoint_url"] = endpoint
    return boto3.resource(
        "dynamodb",
        region_name=os.environ.get("AWS_DEFAULT_REGION", "us-east-1"),
        **kwargs,
    )


def _table(name: str):
    return _client().Table(os.environ[name])


def get_item(table_env: str, key: dict) -> dict | None:
    resp = _table(table_env).get_item(Key=key)
    return resp.get("Item")


def put_item(table_env: str, item: dict, condition: str | None = None) -> bool:
    """Returns True on success, False on ConditionalCheckFailedException."""
    kwargs = {"Item": item}
    if condition:
        kwargs["ConditionExpression"] = condition
    try:
        _table(table_env).put_item(**kwargs)
        return True
    except _client().meta.client.exceptions.ConditionalCheckFailedException:
        return False


def query_gsi(
    table_env: str,
    index_name: str,
    pk_name: str,
    pk_value: str,
    sk_name: str | None = None,
    sk_value: str | None = None,
    scan_forward: bool = True,
    limit: int | None = None,
) -> list[dict]:
    cond = Key(pk_name).eq(pk_value)
    if sk_name and sk_value is not None:
        cond = cond & Key(sk_name).eq(sk_value)
    kwargs = {
        "IndexName": index_name,
        "KeyConditionExpression": cond,
        "ScanIndexForward": scan_forward,
    }
    if limit:
        kwargs["Limit"] = limit
    resp = _table(table_env).query(**kwargs)
    return resp.get("Items", [])


def update_item(
    table_env: str,
    key: dict,
    update_expression: str,
    expression_values: dict,
    condition: str | None = None,
) -> bool:
    """Returns True on success, False on ConditionalCheckFailedException."""
    kwargs = {
        "Key": key,
        "UpdateExpression": update_expression,
        "ExpressionAttributeValues": expression_values,
    }
    if condition:
        kwargs["ConditionExpression"] = condition
    try:
        _table(table_env).update_item(**kwargs)
        return True
    except _client().meta.client.exceptions.ConditionalCheckFailedException:
        return False


def delete_item(table_env: str, key: dict) -> None:
    _table(table_env).delete_item(Key=key)
