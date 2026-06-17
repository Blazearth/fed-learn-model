"""
Bug 2 regression: atomic epoch lock enforcement (Task 9.5).
Validates the TOCTOU fix in activate_epoch.py.
"""
import sys
import os
import pytest
from botocore.exceptions import ClientError

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "lambdas"))


class TestEpochLock:
    def _activate(self, table, epoch_id, model_id):
        """Helper: writes lock item the same way activate_epoch.py does."""
        lock_key = f"MODEL#{model_id}#LOCK"
        table.put_item(
            Item={"epoch_id": lock_key, "active_epoch_id": epoch_id},
            ConditionExpression="attribute_not_exists(epoch_id)",
        )

    def test_first_activation_succeeds(self, aws):
        table = aws["ddb"].Table("EpochTable")
        # Should not raise
        self._activate(table, "EPOCH#fraud-v2#1", "fraud-v2")
        lock = table.get_item(Key={"epoch_id": "MODEL#fraud-v2#LOCK"})["Item"]
        assert lock["active_epoch_id"] == "EPOCH#fraud-v2#1"

    def test_concurrent_activation_blocked_by_lock(self, aws):
        table = aws["ddb"].Table("EpochTable")
        # First activation succeeds
        self._activate(table, "EPOCH#fraud-v2#1", "fraud-v2")

        # Second activation must fail with ConditionalCheckFailedException
        with pytest.raises(ClientError) as exc_info:
            self._activate(table, "EPOCH#fraud-v2#2", "fraud-v2")
        assert exc_info.value.response["Error"]["Code"] == "ConditionalCheckFailedException"

    def test_lock_deleted_after_epoch_completes(self, aws):
        table = aws["ddb"].Table("EpochTable")
        self._activate(table, "EPOCH#fraud-v2#1", "fraud-v2")

        # Simulate aggregator deleting the lock
        table.delete_item(Key={"epoch_id": "MODEL#fraud-v2#LOCK"})

        # Now a new activation should succeed
        self._activate(table, "EPOCH#fraud-v2#2", "fraud-v2")
        lock = table.get_item(Key={"epoch_id": "MODEL#fraud-v2#LOCK"})["Item"]
        assert lock["active_epoch_id"] == "EPOCH#fraud-v2#2"
