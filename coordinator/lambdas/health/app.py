"""GET /api/health — no auth required."""
from datetime import datetime, timezone
from shared.response import ok
import sys, os
sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))


def handler(event, context):
    return ok({"status": "ok", "timestamp": datetime.now(timezone.utc).isoformat()})
