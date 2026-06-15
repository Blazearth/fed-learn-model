"""Standard API Gateway response helpers."""
import json

HEADERS = {"Content-Type": "application/json"}


def ok(body: dict) -> dict:
    return {"statusCode": 200, "headers": HEADERS, "body": json.dumps(body)}


def error(status_code: int, message: str) -> dict:
    return {"statusCode": status_code, "headers": HEADERS,
            "body": json.dumps({"error": message})}


def bad_request(message: str) -> dict:
    return error(400, message)


def forbidden(message: str) -> dict:
    return error(403, message)


def not_found(message: str) -> dict:
    return error(404, message)


def conflict(message: str) -> dict:
    return error(409, message)
