#!/usr/bin/env python3
"""
Generate Ed25519 signing key pair for model signing.
Stores keys in SSM Parameter Store (prod) or local signing_key/ directory (--local).

Usage:
  python scripts/generate_signing_key.py --local
  python scripts/generate_signing_key.py  # writes to SSM
"""
import argparse
import os
import sys

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives.serialization import (
    Encoding, NoEncryption, PrivateFormat, PublicFormat,
)


def main():
    parser = argparse.ArgumentParser(description="Generate Ed25519 signing key pair")
    parser.add_argument("--local", action="store_true",
                        help="Save to coordinator/signing_key/ instead of SSM")
    args = parser.parse_args()

    private_key = Ed25519PrivateKey.generate()
    public_key = private_key.public_key()

    private_pem = private_key.private_bytes(Encoding.PEM, PrivateFormat.PKCS8, NoEncryption())
    public_pem = public_key.public_bytes(Encoding.PEM, PublicFormat.SubjectPublicKeyInfo)

    if args.local:
        key_dir = os.path.join(os.path.dirname(__file__), "..", "signing_key")
        os.makedirs(key_dir, exist_ok=True)
        with open(os.path.join(key_dir, "private.pem"), "wb") as f:
            f.write(private_pem)
        with open(os.path.join(key_dir, "public.pem"), "wb") as f:
            f.write(public_pem)
        print(f"Keys saved to {key_dir}/")
        print("  private.pem — used by aggregator to sign new models")
        print("  public.pem  — embed in daemon config as coordinator public key")
    else:
        import boto3
        ssm = boto3.client("ssm")
        ssm.put_parameter(
            Name="/fl-coordinator/ed25519-private-key",
            Value=private_pem.decode(),
            Type="SecureString",
            Overwrite=True,
        )
        ssm.put_parameter(
            Name="/fl-coordinator/ed25519-public-key",
            Value=public_pem.decode(),
            Type="String",
            Overwrite=True,
        )
        print("Keys stored in SSM Parameter Store:")
        print("  /fl-coordinator/ed25519-private-key  (SecureString)")
        print("  /fl-coordinator/ed25519-public-key   (String)")

    print("\nPublic key (for daemon config.toml coordinator_public_key field):")
    print(public_pem.decode())


if __name__ == "__main__":
    main()
