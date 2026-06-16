#!/usr/bin/env bash
# Upload the CA bundle PEM to S3 so API Gateway mTLS can validate client certs.
# Usage: ./upload_ca_bundle.sh --bucket <ca-bundle-bucket> [--ca-file ./pki/ca.pem]
set -euo pipefail

BUCKET=""
CA_FILE="$(pwd)/pki/ca.pem"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --bucket)  BUCKET="$2";  shift 2 ;;
        --ca-file) CA_FILE="$2"; shift 2 ;;
        *) echo "Unknown argument: $1"; exit 1 ;;
    esac
done

if [[ -z "$BUCKET" ]]; then
    echo "Usage: $0 --bucket <bucket-name> [--ca-file ./pki/ca.pem]"
    exit 1
fi

echo "Uploading $CA_FILE to s3://$BUCKET/ca-bundle.pem ..."
aws s3 cp "$CA_FILE" "s3://$BUCKET/ca-bundle.pem"
echo "Done. API Gateway mTLS truststore updated."
