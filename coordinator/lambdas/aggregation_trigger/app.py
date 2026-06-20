"""
DynamoDB Streams consumer — fires on every INSERT into SubmissionTable.
When submission count >= secure_agg_threshold, transitions epoch ACTIVE → AGGREGATING
and launches the ECS Fargate AggregationTask.
"""
import json
import logging
import os
import sys
import boto3

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

from shared.audit import write_audit_entry
from shared.dynamodb import get_item, query_gsi, update_item

logger = logging.getLogger(__name__)


def handler(event, context):
    for record in event.get("Records", []):
        if record.get("eventName") != "INSERT":
            continue

        new_image = record["dynamodb"].get("NewImage", {})
        epoch_id = _ddb_str(new_image.get("epoch_id"))
        model_id = _ddb_str(new_image.get("model_id"))
        if not epoch_id or not model_id:
            continue

        _process_epoch(epoch_id, model_id)


def _process_epoch(epoch_id: str, model_id: str) -> None:
    # 1. Fetch epoch
    epoch = get_item("EPOCH_TABLE", {"epoch_id": epoch_id})
    if not epoch:
        logger.warning("Epoch %s not found", epoch_id)
        return

    # 2. Idempotency — only proceed if still ACTIVE
    if epoch.get("status") != "ACTIVE":
        logger.info("Epoch %s status=%s — skipping", epoch_id, epoch.get("status"))
        return

    # 3. Count submissions
    submissions = query_gsi(
        "SUBMISSION_TABLE",
        "epoch_id-org_id-index",
        pk_name="epoch_id",
        pk_value=epoch_id,
    )
    count = len(submissions)
    threshold = int(epoch.get("secure_agg_threshold", 1))
    required = max(2, threshold)

    logger.info("Epoch %s: %d/%d submissions", epoch_id, count, required)

    if count < required:
        logger.info(
            "Epoch %s waiting (%d/%d submissions) — not launching ECS yet",
            epoch_id, count, required,
        )
        return

    # 4. Atomically transition ACTIVE → AGGREGATING (Bug 2 fix: conditional update)
    updated = update_item(
        "EPOCH_TABLE",
        key={"epoch_id": epoch_id},
        update_expression="SET #s = :agg",
        expression_values={":agg": "AGGREGATING", ":active": "ACTIVE"},
        condition="#s = :active",
        expression_names={"#s": "status"},
    )
    if not updated:
        logger.info("Epoch %s already moved past ACTIVE — skipping duplicate trigger", epoch_id)
        return

    # 5. Write audit entry
    write_audit_entry(
        model_id=model_id,
        epoch_number=int(epoch.get("epoch_number", 0)),
        event_type="AGGREGATION_TRIGGERED",
        org_id="SYSTEM",
        payload=json.dumps({"submission_count": count, "threshold": threshold}),
    )

    # 6. Launch ECS Fargate task (or local aggregation in LOCAL_MODE)
    if os.environ.get("LOCAL_MODE") == "true":
        _run_local_aggregation(epoch_id, model_id, submissions)
    else:
        _launch_fargate_task(epoch_id, model_id)


def _launch_fargate_task(epoch_id: str, model_id: str) -> None:
    ecs = boto3.client("ecs", region_name=os.environ.get("AWS_DEFAULT_REGION", "us-east-1"))

    # Bug 1 fix: support multiple subnets via comma-separated SUBNET_IDS
    subnet_ids_raw = os.environ.get("SUBNET_IDS", "")
    subnets = [s.strip() for s in subnet_ids_raw.split(",") if s.strip()]
    if not subnets:
        logger.error("SUBNET_IDS env var is empty — cannot launch Fargate task")
        raise RuntimeError("SUBNET_IDS must be set with at least one subnet ID")

    cluster = os.environ.get("ECS_CLUSTER", "FederatedLearningCluster")
    task_def = os.environ.get("ECS_TASK_DEFINITION", "fl-aggregation-worker")

    import time
    last_exc = None
    for attempt in range(3):
        try:
            ecs.run_task(
                cluster=cluster,
                taskDefinition=task_def,
                launchType="FARGATE",
                networkConfiguration={
                    "awsvpcConfiguration": {
                        "subnets": subnets,
                        "assignPublicIp": "ENABLED",
                    }
                },
                overrides={
                    "containerOverrides": [{
                        "name": "aggregation-worker",
                        "environment": [
                            {"name": "EPOCH_ID", "value": epoch_id},
                            {"name": "MODEL_ID", "value": model_id},
                        ],
                    }]
                },
            )
            logger.info(
                "Launched Fargate aggregation task for epoch=%s cluster=%s subnets=%s",
                epoch_id, cluster, subnets,
            )
            return
        except Exception as exc:
            last_exc = exc
            logger.warning("ecs.run_task attempt %d failed: %s", attempt + 1, exc)
            time.sleep(2 ** attempt)   # 1s, 2s, 4s

    raise RuntimeError(f"ecs.run_task failed after 3 attempts: {last_exc}")


def _run_local_aggregation(epoch_id: str, model_id: str, submissions: list) -> None:
    """
    In LOCAL_MODE run the aggregation in-process (no Docker/ECS needed).
    Imports the aggregator module directly.
    """
    import importlib.util
    agg_path = os.path.join(os.path.dirname(__file__), "..", "..", "aggregation", "aggregator.py")
    spec = importlib.util.spec_from_file_location("aggregator", agg_path)
    agg_module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(agg_module)
    agg_module.run_aggregation(epoch_id=epoch_id, model_id=model_id)
    logger.info("Local aggregation completed for epoch %s", epoch_id)


def _ddb_str(attr) -> str:
    if attr is None:
        return ""
    if isinstance(attr, dict):
        return attr.get("S", "")
    return str(attr)
