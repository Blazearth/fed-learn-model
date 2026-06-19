#!/usr/bin/env python3
"""
Register an organization in OrgTable.

Supports two workflows:

1. With a certificate file (recommended — extracts public key automatically):
   python scripts/register_org.py \
       --org-id org-acme-bank \
       --display-name "ACME Bank" \
       --cert-file pki/org-acme-bank.pem

2. With a raw base64 public key string:
   python scripts/register_org.py \
       --org-id org-acme-bank \
       --display-name "ACME Bank" \
       --public-key "<base64>"

3. Local (Docker Compose) mode — add --local flag:
   python scripts/register_org.py --org-id org-acme-bank \
       --display-name "ACME Bank" --cert-file pki/org-acme-bank.pem --local

The org_id must exactly match the CN field in the issued certificate
(e.g. if cert was issued with --org-id org-acme-bank, the CN is org-acme-bank).
"""
import argparse
import base64
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


def _extract_public_key_from_cert(cert_path: str) -> str:
    """
    Load an X.509 PEM certificate and return the DER-encoded public key
    as a base64 string for storage in OrgTable.

    The daemon uses this to verify model signatures and for secure aggregation
    key exchange. Storing the raw public key bytes (not the full cert) keeps
    the OrgTable record compact and algorithm-agnostic.
    """
    from cryptography import x509
    from cryptography.hazmat.primitives.serialization import Encoding, PublicFormat

    with open(cert_path, "rb") as f:
        pem_data = f.read()

    cert = x509.load_pem_x509_certificate(pem_data)
    pub_key_der = cert.public_key().public_bytes(Encoding.DER, PublicFormat.SubjectPublicKeyInfo)
    return base64.b64encode(pub_key_der).decode()


def _verify_cn_matches(cert_path: str, org_id: str) -> None:
    """Warn if the certificate CN does not match the given org_id."""
    from cryptography import x509
    from cryptography.x509.oid import NameOID

    with open(cert_path, "rb") as f:
        cert = x509.load_pem_x509_certificate(f.read())

    cn_attrs = cert.subject.get_attributes_for_oid(NameOID.COMMON_NAME)
    if not cn_attrs:
        print("WARNING: Certificate has no CN field.", file=sys.stderr)
        return
    cn = cn_attrs[0].value
    if cn != org_id:
        print(
            f"WARNING: Certificate CN='{cn}' does not match --org-id='{org_id}'.\n"
            f"         The coordinator uses the CN as the org identity. Requests\n"
            f"         from this cert will be looked up as '{cn}', not '{org_id}'.",
            file=sys.stderr,
        )


def main():
    parser = argparse.ArgumentParser(
        description="Register an organisation in OrgTable",
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument("--org-id", required=True,
                        help="Organisation identifier — must match the CN in the mTLS cert")
    parser.add_argument("--display-name", required=True,
                        help="Human-readable name for the organisation")
    parser.add_argument("--cert-file", default="",
                        help="Path to the org's PEM certificate — public key is extracted automatically")
    parser.add_argument("--public-key", default="",
                        help="Base64-encoded public key (alternative to --cert-file)")
    parser.add_argument("--local", action="store_true",
                        help="Write to DynamoDB Local instead of AWS")
    args = parser.parse_args()

    # Resolve public key
    public_key = ""
    if args.cert_file:
        if not os.path.exists(args.cert_file):
            print(f"ERROR: Certificate file not found: {args.cert_file}", file=sys.stderr)
            sys.exit(1)
        _verify_cn_matches(args.cert_file, args.org_id)
        public_key = _extract_public_key_from_cert(args.cert_file)
        print(f"Extracted public key from {args.cert_file} ({len(public_key)} base64 chars)")
    elif args.public_key:
        public_key = args.public_key
    else:
        print(
            "WARNING: No --cert-file or --public-key provided.\n"
            "         OrgTable will store empty public_key.\n"
            "         Secure aggregation participants will have no key material.\n"
            "         Run again with --cert-file pki/<org-id>.pem to fix this.",
            file=sys.stderr,
        )

    table_name = os.environ.get("ORG_TABLE", "FederatedOrgTable")
    ddb = _ddb(args.local)
    table = ddb.Table(table_name)

    item = {
        "org_id": args.org_id,
        "display_name": args.display_name,
        "status": "ACTIVE",
        "public_key": public_key,
        "registered_at": datetime.now(timezone.utc).isoformat(),
    }
    table.put_item(Item=item)

    print(f"Registered org '{args.org_id}' ({args.display_name}) as ACTIVE")
    if public_key:
        print(f"  public_key: {public_key[:32]}...")
    else:
        print("  public_key: (empty — secure aggregation metadata will lack key material)")


if __name__ == "__main__":
    main()
