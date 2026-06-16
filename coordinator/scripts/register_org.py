#!/usr/bin/env python3
"""
Register an organization in OrgTable.

Usage:
  python scripts/register_org.py --org-id org-acme-bank --display-name "ACME Bank" --local
  python scripts/register_org.py --org-id org-acme-bank --display-name "ACME Bank"
"""
import argparse
import os
import sys
from datetime import datetime, timezone

import boto3


def _ddb(local: bool):
    kwargs = {"region_name": "us-east-1"}
    if local:
        kwargs["endpoint_url"] = os.environ.get("DYNAMODB_ENDPOINT", "http://localhost:8000")
        os.environ.setdefault("AWS_ACCESS_KEY_ID", "test")
        os.environ.setdefault("AWS_SECRET_ACCESS_KEY", "test")
    return boto3.resource("dynamodb", **kwargs)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--org-id", required=True)
    parser.add_argument("--display-name", required=True)
    parser.add_argument("--public-key", default="", help="Ed25519 public key (base64)")
    parser.add_argument("--local", action="store_true")
    args = parser.parse_args()

    table_name = os.environ.get("ORG_TABLE", "FederatedOrgTable")
    ddb = _ddb(args.local)
    table = ddb.Table(table_name)

    item = {
        "org_id": args.org_id,
        "display_name": args.display_name,
        "status": "ACTIVE",
        "public_key": args.public_key,
        "registered_at": datetime.now(timezone.utc).isoformat(),
    }
    table.put_item(Item=item)
    print(f"Registered org '{args.org_id}' ({args.display_name}) as ACTIVE")


if __name__ == "__main__":
    main()
