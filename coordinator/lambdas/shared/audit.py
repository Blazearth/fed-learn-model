"""
Tamper-evident audit log writer with SHA-256 hash chaining.
Failures are non-blocking — logged to stdout, never raised.
"""
import hashlib
import json
import logging
import os
from datetime import datetime, timezone

from .dynamodb import get_item, put_item, query_gsi

logger = logging.getLogger(__name__)
GENESIS_HASH = "0" * 64


def _ulid() -> str:
    """Simple time-sortable ID: timestamp_ms (10 hex) + random (16 hex)."""
    import time, random
    ts = int(time.time() * 1000)
    rand = random.getrandbits(64)
    return f"AUDIT#{ts:013x}{rand:016x}"


def _utc_now() -> str:
    return datetime.now(timezone.utc).isoformat()


def write_audit_entry(
    model_id: str,
    epoch_number: int,
    event_type: str,
    org_id: str,
    payload: str,
) -> None:
    """Write one audit entry. Never raises — failures are logged only."""
    try:
        # 1. Get previous hash (last entry for this model_id, newest first)
        previous_hash = GENESIS_HASH
        recent = query_gsi(
            "AUDIT_TABLE",
            "model_id-created_at-index",
            pk_name="model_id",
            pk_value=model_id,
            scan_forward=False,
            limit=1,
        )
        if recent:
            previous_hash = recent[0].get("entry_hash", GENESIS_HASH)

        # 2. Build entry
        entry_id = _ulid()
        created_at = _utc_now()
        raw = f"{entry_id}{event_type}{org_id}{payload}{previous_hash}"
        entry_hash = hashlib.sha256(raw.encode()).hexdigest()

        item = {
            "entry_id": entry_id,
            "model_id": model_id,
            "epoch_number": epoch_number,
            "event_type": event_type,
            "org_id": org_id,
            "payload": payload,
            "previous_hash": previous_hash,
            "entry_hash": entry_hash,
            "created_at": created_at,
        }

        # 3. Write with idempotency guard
        put_item("AUDIT_TABLE", item, condition="attribute_not_exists(entry_id)")
        logger.info("audit entry written: %s %s", event_type, entry_id)

    except Exception as exc:  # noqa: BLE001
        logger.error("audit write failed (non-blocking): %s", exc)
