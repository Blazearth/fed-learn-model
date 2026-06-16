#!/usr/bin/env bash
# Generate the FL Platform root CA (run once by the platform operator).
# Outputs: ca.key (keep offline!), ca.pem (upload to S3 + distribute to orgs)
set -euo pipefail

OUT_DIR="${1:-$(pwd)/pki}"
mkdir -p "$OUT_DIR"

echo "Generating 4096-bit RSA root CA key..."
openssl genrsa -out "$OUT_DIR/ca.key" 4096

echo "Generating self-signed CA certificate (10-year validity)..."
openssl req -x509 -new -nodes \
    -key "$OUT_DIR/ca.key" \
    -sha256 \
    -days 3650 \
    -out "$OUT_DIR/ca.pem" \
    -subj "/CN=FL-Platform-CA/O=FL Platform/C=US"

echo ""
echo "Done. Files created in $OUT_DIR/"
echo "  ca.key  — KEEP THIS OFFLINE AND SECURE. Never commit it."
echo "  ca.pem  — Upload to S3 (CaBundleBucket) and distribute to participating orgs."
echo ""
echo "Next step:"
echo "  aws s3 cp $OUT_DIR/ca.pem s3://<ca-bundle-bucket>/ca-bundle.pem"
