# Federated Learning Coordinator Final Deployment and Production Test Report

## 1. Executive Summary

This report documents the complete deployment and production validation of the Federated Learning Coordinator platform on AWS. The system now supports:

- mutual TLS (mTLS) authentication for organizations,
- organization onboarding and identity enforcement,
- epoch lifecycle management,
- model download and update submission APIs,
- S3-backed update verification,
- DynamoDB Streams based aggregation triggers,
- ECS Fargate aggregation workers,
- model publication to S3,
- hash-chained audit logging,
- CloudWatch metrics, and
- automatic creation of the next training epoch.

The platform completed a full two-organization federated learning round using `org-aiims` and `org-kgmu`, producing a new global model `v3/model.npy` and creating `Epoch #3` as `PENDING`.

The earlier deployment report stopped after infrastructure setup and mTLS verification. This version includes the full working workflow from start to end. The earlier report is now superseded. fileciteturn13file0turn13file1

## 2. Final Working State

The system is currently working end-to-end with the following confirmed results:

- The custom domain `coordinator.fed-learn.online` resolves correctly and serves the coordinator API.
- mTLS is enforced successfully through API Gateway with the uploaded CA truststore.
- `org-aiims` and `org-kgmu` are both registered in `FederatedOrgTable` and marked `ACTIVE`.
- `Epoch #2` was activated, accepted two submissions, and completed successfully.
- The aggregation worker used the deployed container image from ECR and completed the round.
- A new model version `v3` was written to S3.
- CloudWatch shows `AggregationSuccess` for `fraud-detection-v2`.
- The next epoch `EPOCH#fraud-detection-v2#3` was created as `PENDING`.

## 3. AWS Environment and Core Identifiers

| Item | Value |
|---|---|
| AWS Account ID | `318629836373` |
| AWS Region | `us-east-1` |
| Primary custom domain | `https://coordinator.fed-learn.online` |
| API Gateway domain name | `d-orzgespo9f.execute-api.us-east-1.amazonaws.com` |
| HTTP API ID | `7aevhl2yy4` |
| API mapping ID | `wrq6v3` |
| ECR repository | `fl-aggregation-worker` |
| ECR URI | `318629836373.dkr.ecr.us-east-1.amazonaws.com/fl-aggregation-worker` |
| Ingestion bucket | `fl-ingestion-318629836373-dev` |
| CA bundle bucket | `fl-ca-bundle-318629836373-dev` |
| Main model ID | `fraud-detection-v2` |

### Important note on the server point to hit

Use the custom domain for all normal client requests:

```bash
https://coordinator.fed-learn.online
```

The raw API Gateway hostname exists, but the custom domain is the intended endpoint for the mTLS flow.

## 4. AWS Resources Provisioned

### 4.1 Lambda Functions

The coordinator stack deploys the following Lambda handlers:

| Function | Path | Purpose |
|---|---|---|
| `HealthFunction` | `GET /api/health` | Public health check |
| `EpochQueryFunction` | `GET /api/epochs/active` | Returns the active epoch metadata for a model |
| `ModelUrlFunction` | `POST /api/models/download-url` | Generates pre-signed S3 GET URLs for global models |
| `UpdateUrlFunction` | `POST /api/updates/upload-url` | Generates pre-signed S3 PUT URLs for update uploads |
| `UpdateCompleteFunction` | `POST /api/updates/complete` | Validates an uploaded update and records a submission |
| `AggregationTriggerFunction` | DynamoDB Stream consumer | Launches aggregation when enough submissions are present |
| `AuditQueryFunction` | `GET /api/audit` | Returns audit trail data |

### 4.2 DynamoDB Tables

| Table | Purpose |
|---|---|
| `FederatedEpochTable` | Stores epoch state, model versions, hashes, signatures, activation/completion timestamps, and the lock item |
| `FederatedSubmissionTable` | Stores update submissions received from organizations |
| `FederatedAuditTable` | Stores hash-chained audit records |
| `FederatedOrgTable` | Stores organization identity, public key, status, and registration timestamp |

### 4.3 S3 Buckets

| Bucket | Purpose |
|---|---|
| `fl-ingestion-318629836373-dev` | Stores model binaries and client update binaries |
| `fl-ca-bundle-318629836373-dev` | Stores the mTLS truststore `ca-bundle.pem` |

### 4.4 ECS and ECR

| Resource | Value |
|---|---|
| ECS cluster | `FederatedLearningCluster` |
| ECS task definition family | `fl-aggregation-worker` |
| Container image | `318629836373.dkr.ecr.us-east-1.amazonaws.com/fl-aggregation-worker:latest` |
| ECS launch type | Fargate |

### 4.5 CloudWatch

| Item | Value |
|---|---|
| Namespace | `FederatedLearning` |
| Log group for aggregation | `/fl-coordinator/aggregation` |
| Log group for UpdateComplete Lambda | `/aws/lambda/fl-coordinator-UpdateCompleteFunction-DUuP7X2aIX4M` |
| Log group for AggregationTrigger Lambda | `/aws/lambda/fl-coordinator-AggregationTriggerFunction-1ppJ1wEEiB76` |

## 5. Security and Identity Architecture

### 5.1 Certificate Authority

A private root CA was created locally and used to issue organization certificates.

Files:

- `pki/ca.key`
- `pki/ca.pem`

The CA certificate was uploaded as the truststore to S3:

- `s3://fl-ca-bundle-318629836373-dev/ca-bundle.pem`

### 5.2 mTLS

API Gateway custom domain `coordinator.fed-learn.online` is configured with mTLS.

Behavior confirmed during testing:

- requests without a client certificate are rejected,
- requests with the certificate and key from a registered organization succeed,
- the truststore validates the client certificate chain correctly.

### 5.3 Organization Certificates

Two organization certificates were issued and registered:

- `org-aiims`
- `org-kgmu`

Each organization has:

- a PEM certificate,
- a private key,
- a corresponding row in `FederatedOrgTable`,
- `status = ACTIVE`,
- a stored public key extracted from the certificate.

### 5.4 Authentication Logic

For production requests, the Lambda auth layer reads the client certificate from the API Gateway request context. The org identity is extracted from the certificate subject CN. The organization must exist in `FederatedOrgTable` and must have `status = ACTIVE`.

## 6. Database Design

### 6.1 FederatedEpochTable

Key attributes observed in the workflow:

- `epoch_id`  
  Primary key. Examples:
  - `EPOCH#fraud-detection-v2#1`
  - `EPOCH#fraud-detection-v2#2`
  - `EPOCH#fraud-detection-v2#3`

- `model_id`  
  Example: `fraud-detection-v2`

- `epoch_number`  
  Numeric epoch counter.

- `status`  
  Lifecycle state:
  - `PENDING`
  - `ACTIVE`
  - `AGGREGATING`
  - `COMPLETED`
  - `FAILED`

- `model_version`  
  Example: `v1`, `v2`, `v3`

- `model_hash`  
  SHA-256 hash of the `.npy` model file.

- `model_signature`  
  Base64-encoded Ed25519 signature over the model bytes.

- `model_s3_key`  
  Example:
  - `models/fraud-detection-v2/v1/model.npy`
  - `models/fraud-detection-v2/v2/model.npy`
  - `models/fraud-detection-v2/v3/model.npy`

- `architecture_hash`
- `fedprox_mu`
- `privacy_epsilon`
- `privacy_delta`
- `secure_agg_threshold`
- `dataset_schema`
- `drift_alerts`
- `created_at`
- `activated_at`
- `completed_at`

The table also stores the lock item:

- `epoch_id = MODEL#fraud-detection-v2#LOCK`
- `active_epoch_id = EPOCH#fraud-detection-v2#<n>`
- `activated_at`

### 6.2 FederatedSubmissionTable

Observed attributes:

- `submission_id`
- `epoch_id`
- `org_id`
- `model_id`
- `epoch_number`
- `update_hash`
- `s3_key`
- `submitted_at`
- `status`

Example S3 key format:

```text
updates/{model_id}/{epoch_number}/{org_id}/update.bin
```

### 6.3 FederatedAuditTable

Observed attributes:

- `entry_id`
- `model_id`
- `epoch_number`
- `event_type`
- `org_id`
- `payload`
- `previous_hash`
- `entry_hash`
- `created_at`

Important event types observed:

- `UPDATE_SUBMITTED`
- `AGGREGATION_TRIGGERED`
- `MODEL_PUBLISHED`

The audit chain now links entries using `previous_hash` instead of restarting from zeros for every new event.

### 6.4 FederatedOrgTable

Observed attributes:

- `org_id`
- `display_name`
- `status`
- `public_key`
- `registered_at`

Example values:

- `org-aiims`
- `AIIMS Test Organisation`
- `ACTIVE`

- `org-kgmu`
- `KGMU Test Organisation`
- `ACTIVE`

## 7. API Surface and Where to Hit

### Base URL

Use the custom mTLS domain:

```text
https://coordinator.fed-learn.online
```

### Health Check

**Method:** `GET`  
**Path:** `/api/health`  
**Auth:** none required  
**Purpose:** health/status probe

Example:

```bash
curl -I https://coordinator.fed-learn.online/api/health
```

Successful response:

```json
{"status":"ok","timestamp":"..."}
```

### Get Active Epoch

**Method:** `GET`  
**Path:** `/api/epochs/active`  
**Query parameters:** `model_id` required  
**Auth:** mTLS client certificate required  
**Purpose:** retrieve the currently active epoch metadata for a model

Example:

```bash
curl \
  --cert pki/org-aiims.pem \
  --key pki/org-aiims.key \
  --cacert pki/ca.pem \
  "https://coordinator.fed-learn.online/api/epochs/active?model_id=fraud-detection-v2"
```

Typical response fields:

- `epoch_number`
- `model_id`
- `model_version`
- `model_hash`
- `model_signature`
- `architecture_hash`
- `fedprox_mu`
- `privacy_epsilon`
- `privacy_delta`
- `secure_agg_participants`
- `secure_agg_threshold`
- `drift_alerts`
- `dataset_schema`

### Get Model Download URL

**Method:** `POST`  
**Path:** `/api/models/download-url`  
**Auth:** mTLS client certificate required  
**Body:**

```json
{
  "model_id": "fraud-detection-v2",
  "model_version": "v2"
}
```

**Purpose:** returns a pre-signed S3 GET URL for the current global model file.

Example:

```bash
curl \
  --cert pki/org-aiims.pem \
  --key pki/org-aiims.key \
  --cacert pki/ca.pem \
  -H "Content-Type: application/json" \
  -X POST \
  https://coordinator.fed-learn.online/api/models/download-url \
  -d '{"model_id":"fraud-detection-v2","model_version":"v2"}'
```

### Get Update Upload URL

**Method:** `POST`  
**Path:** `/api/updates/upload-url`  
**Auth:** mTLS client certificate required  
**Body:**

```json
{
  "model_id": "fraud-detection-v2",
  "epoch_number": 2
}
```

**Purpose:** returns a pre-signed S3 PUT URL for a deterministic update binary key.

Deterministic S3 key:

```text
updates/fraud-detection-v2/2/org-aiims/update.bin
```

Example:

```bash
curl \
  --cert pki/org-aiims.pem \
  --key pki/org-aiims.key \
  --cacert pki/ca.pem \
  -H "Content-Type: application/json" \
  -X POST \
  https://coordinator.fed-learn.online/api/updates/upload-url \
  -d '{"model_id":"fraud-detection-v2","epoch_number":2}'
```

### Submit Update Completion

**Method:** `POST`  
**Path:** `/api/updates/complete`  
**Auth:** mTLS client certificate required  
**Body:**

```json
{
  "model_id": "fraud-detection-v2",
  "epoch_number": 2,
  "update_hash": "..."
}
```

**Purpose:** validates the uploaded file exists in S3, enforces size bounds, verifies the SHA-256 hash, and writes a submission row.

Validation performed by the deployed Lambda:

- update file must exist in S3,
- file size must be between 1 KB and 500 MB,
- computed SHA-256 must match the claimed `update_hash`,
- duplicate submission by the same org for the same epoch is rejected.

Example:

```bash
curl \
  --cert pki/org-aiims.pem \
  --key pki/org-aiims.key \
  --cacert pki/ca.pem \
  -H "Content-Type: application/json" \
  -X POST \
  https://coordinator.fed-learn.online/api/updates/complete \
  -d '{
    "model_id":"fraud-detection-v2",
    "epoch_number":2,
    "update_hash":"290c839800f7cbb27ccf0c1a72f96943ef453ba35276af9aa772cb27b181ccb4"
  }'
```

### Audit Query

**Method:** `GET`  
**Path:** `/api/audit`  
**Auth:** mTLS client certificate required  
**Purpose:** inspect audit trail entries for a model

Example pattern:

```bash
curl \
  --cert pki/org-aiims.pem \
  --key pki/org-aiims.key \
  --cacert pki/ca.pem \
  "https://coordinator.fed-learn.online/api/audit?model_id=fraud-detection-v2&limit=20"
```

## 8. Developer and Operator Workflow

### 8.1 Container Build and Push

The aggregation worker image must be rebuilt and pushed when `aggregation/aggregator.py` changes.

Typical flow:

```bash
docker build \
  -t fl-aggregation-worker:latest \
  -f aggregation/Dockerfile \
  aggregation/

docker tag fl-aggregation-worker:latest \
  318629836373.dkr.ecr.us-east-1.amazonaws.com/fl-aggregation-worker:latest

docker push \
  318629836373.dkr.ecr.us-east-1.amazonaws.com/fl-aggregation-worker:latest
```

### 8.2 SAM Build and Deploy

Whenever Lambda code or the CloudFormation/SAM template changes:

```bash
sam build
sam deploy
```

### 8.3 Certificate Issuance and Registration

Issue a certificate:

```bash
./scripts/issue_org_cert.sh --org-id org-kgmu
```

Register the organization:

```bash
python scripts/register_org.py \
  --org-id org-kgmu \
  --display-name "KGMU Test Organisation" \
  --cert-file pki/org-kgmu.pem
```

### 8.4 Seed and Activate Epochs

Seed a new epoch:

```bash
python scripts/seed_epoch.py \
  --model-id fraud-detection-v2 \
  --epoch-number 2 \
  --threshold 1
```

Activate an epoch:

```bash
python scripts/activate_epoch.py \
  --epoch-id "EPOCH#fraud-detection-v2#2"
```

## 9. Successful End-to-End Production Round

The platform successfully completed a real round with two organizations.

### 9.1 Initial State

- `org-aiims` registered and active.
- `org-kgmu` registered and active.
- `EPOCH#fraud-detection-v2#2` activated.
- Lock item created:
  - `MODEL#fraud-detection-v2#LOCK -> EPOCH#fraud-detection-v2#2`

### 9.2 AIIMS Submission

AIIMS uploaded the file:

```text
updates/fraud-detection-v2/2/org-aiims/update.bin
```

The update was larger than the minimum size requirement and passed all checks. Submission row created in `FederatedSubmissionTable`.

Lambda state after the first submission:

- count = 1
- required = 2
- epoch remained `ACTIVE`
- no aggregation launched yet

### 9.3 KGMU Submission

KGMU uploaded the file:

```text
updates/fraud-detection-v2/2/org-kgmu/update.bin
```

The update was also larger than the minimum size requirement and passed all checks.

Once the second submission arrived:

- `AggregationTriggerFunction` detected `2/2` submissions,
- epoch transitioned `ACTIVE -> AGGREGATING`,
- ECS Fargate task was launched,
- aggregation worker downloaded both updates,
- `FedAvg` fallback was used because `n=2` is below the secure aggregation threshold `2f+3=3`,
- the new global model was uploaded as `models/fraud-detection-v2/v3/model.npy`,
- epoch `#2` was marked `COMPLETED`,
- epoch `#3` was created as `PENDING`,
- the lock was released,
- a `MODEL_PUBLISHED` audit entry was written,
- CloudWatch metric `AggregationSuccess` was emitted.

### 9.4 Final Observable Outputs

DynamoDB Epoch Table showed:

- `EPOCH#fraud-detection-v2#2` -> `COMPLETED`
- `EPOCH#fraud-detection-v2#3` -> `PENDING`

S3 showed:

- `v1/model.npy`
- `v2/model.npy`
- `v3/model.npy`

CloudWatch showed:

- `AggregationSuccess` with dimension `ModelId=fraud-detection-v2`

Aggregation logs showed:

- both updates loaded successfully,
- FedAvg fallback used,
- model published,
- lock released,
- next epoch created.

## 10. Validation and Operational Observations

### 10.1 mTLS Validation

Testing without a client certificate resulted in a TLS reset.  
Testing with `org-aiims` certificate and CA bundle succeeded and returned `HTTP 200` on `/api/health`.

### 10.2 Size Validation

A 640-byte test update was rejected because the deployed Lambda enforces a minimum file size of 1024 bytes. Larger updates (for example, 4096 float32 values, producing a 16 KB `.npy` file) succeeded.

### 10.3 Aggregation Behavior

The deployed worker correctly performs a fallback to FedAvg when the number of updates is too small for the stronger secure aggregation path.

### 10.4 Metrics

`AggregationSuccess` is visible in CloudWatch Metrics under the `FederatedLearning` namespace, confirming that the metrics path is wired correctly.

## 11. Current Status

### Working

- custom domain
- ACM certificate
- CA and truststore
- mTLS
- organization onboarding
- protected API endpoints
- epoch activation
- model download URL generation
- update upload URL generation
- update validation
- duplicate prevention
- DynamoDB stream trigger
- ECS aggregation task
- model publication to S3
- audit logging
- CloudWatch metrics
- next epoch creation

### Confirmed Artifacts

- `org-aiims`
- `org-kgmu`
- `v1`, `v2`, `v3` model versions
- `Epoch #2 COMPLETED`
- `Epoch #3 PENDING`

## 12. Conclusion

The Federated Learning Coordinator platform is now fully operational on AWS and has been validated beyond infrastructure setup. It successfully completed a real two-organization federated learning round using authenticated clients, secure cloud storage, DynamoDB-based orchestration, Lambda-triggered aggregation, ECS Fargate execution, and metric/audit instrumentation.

The system is no longer just deployed. It is working end-to-end.

