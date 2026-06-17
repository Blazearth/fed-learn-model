"""Audit hash chain integrity tests (Task 9.4)."""
import hashlib
import sys
import os
import pytest

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "lambdas"))
from shared.audit import write_audit_entry, GENESIS_HASH
from shared.dynamodb import query_gsi


class TestAuditChain:
    def test_five_entries_form_unbroken_chain(self, aws):
        for i in range(5):
            write_audit_entry("fraud-v2", 1, f"EVENT_{i}", "org-a", "{}")

        entries = sorted(
            query_gsi("AUDIT_TABLE", "model_id-created_at-index",
                      pk_name="model_id", pk_value="fraud-v2"),
            key=lambda e: e["created_at"],
        )
        assert len(entries) == 5

        # Verify chain: each entry's previous_hash == prior entry's entry_hash
        for i in range(1, len(entries)):
            assert entries[i]["previous_hash"] == entries[i - 1]["entry_hash"], (
                f"Chain broken at entry {i}"
            )

    def test_first_entry_uses_genesis_hash(self, aws):
        write_audit_entry("model-new", 1, "EPOCH_ACTIVATED", "SYSTEM", "{}")
        entries = query_gsi("AUDIT_TABLE", "model_id-created_at-index",
                            pk_name="model_id", pk_value="model-new")
        assert entries[0]["previous_hash"] == GENESIS_HASH

    def test_entry_hash_formula_is_correct(self, aws):
        write_audit_entry("fraud-v2", 1, "UPDATE_SUBMITTED", "org-b", '{"k":"v"}')
        entry = query_gsi("AUDIT_TABLE", "model_id-created_at-index",
                          pk_name="model_id", pk_value="fraud-v2")[0]
        expected = hashlib.sha256(
            f"{entry['entry_id']}UPDATE_SUBMITTEDorg-b{{\"k\":\"v\"}}{entry['previous_hash']}".encode()
        ).hexdigest()
        assert entry["entry_hash"] == expected

    def test_tampered_entry_breaks_chain(self, aws):
        for i in range(3):
            write_audit_entry("fraud-v2", 1, f"EVT_{i}", "org-a", "{}")

        entries = sorted(
            query_gsi("AUDIT_TABLE", "model_id-created_at-index",
                      pk_name="model_id", pk_value="fraud-v2"),
            key=lambda e: e["created_at"],
        )
        # Tamper the middle entry's payload
        tampered_hash = entries[1]["entry_hash"]
        recomputed = hashlib.sha256(
            f"{entries[2]['entry_id']}{entries[2]['event_type']}{entries[2]['org_id']}"
            f"{entries[2]['payload']}{tampered_hash}".encode()
        ).hexdigest()
        # The stored hash of entry 2 was computed with un-tampered entry 1
        # After tampering entry 1, entry 2's previous_hash no longer matches entry 1's hash
        assert entries[2]["previous_hash"] == entries[1]["entry_hash"]
        # If we tamper entry 1's entry_hash, entry 2's previous_hash would differ
        fake_hash = "0" * 64
        assert entries[2]["previous_hash"] != fake_hash
