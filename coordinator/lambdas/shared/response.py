"""Standard API Gateway response helpers."""
import json
from decimal import Decimal

HEADERS = {"Content-Type": "application/json"}


class _DecimalEncoder(json.JSONEncoder):
    def default(self, o):
        if isinstance(o, Decimal):
            return int(o) if o % 1 == 0 else float(o)
        return super().default(o)


def ok(body: dict) -> dict:
    return {"statusCode": 200, "headers": HEADERS,
            "body": json.dumps(body, cls=_DecimalEncoder)}


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
