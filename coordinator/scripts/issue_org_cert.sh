#!/usr/bin/env bash
# Issue an X.509 client certificate for one participating organization.
# Usage: ./issue_org_cert.sh --org-id org-acme-bank [--ca-dir ./pki] [--out-dir ./pki]
set -euo pipefail

ORG_ID=""
CA_DIR="$(pwd)/pki"
OUT_DIR="$(pwd)/pki"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --org-id)  ORG_ID="$2";  shift 2 ;;
        --ca-dir)  CA_DIR="$2";  shift 2 ;;
        --out-dir) OUT_DIR="$2"; shift 2 ;;
        *) echo "Unknown argument: $1"; exit 1 ;;
    esac
done

if [[ -z "$ORG_ID" ]]; then
    echo "Usage: $0 --org-id <org-id> [--ca-dir ./pki] [--out-dir ./pki]"
    exit 1
fi

mkdir -p "$OUT_DIR"

echo "Generating 2048-bit RSA key for $ORG_ID..."
openssl genrsa -out "$OUT_DIR/${ORG_ID}.key" 2048

echo "Generating CSR with CN=$ORG_ID..."
openssl req -new -key "$OUT_DIR/${ORG_ID}.key" \
    -out "$OUT_DIR/${ORG_ID}.csr" \
    -subj "/CN=${ORG_ID}/O=FL Platform Participant/C=US"

echo "Signing certificate with root CA (365-day validity)..."
openssl x509 -req \
    -in "$OUT_DIR/${ORG_ID}.csr" \
    -CA "$CA_DIR/ca.pem" \
    -CAkey "$CA_DIR/ca.key" \
    -CAcreateserial \
    -out "$OUT_DIR/${ORG_ID}.pem" \
    -days 365 \
    -sha256

rm "$OUT_DIR/${ORG_ID}.csr"

echo ""
echo "Done. Files created:"
echo "  ${ORG_ID}.pem — Send this to the organization (their client certificate)"
echo "  ${ORG_ID}.key — The organization loads this into their TPM/HSM"
echo ""
echo "Next step:"
echo "  python scripts/register_org.py --org-id $ORG_ID --public-key-file $OUT_DIR/${ORG_ID}.pem"
