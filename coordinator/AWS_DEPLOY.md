# AWS Deployment Guide — FL Cloud Coordinator

This document covers everything you need to deploy the Cloud Coordinator to AWS from scratch.
It mirrors exactly what the codebase does: SAM for Lambda + API Gateway, ECR for the
aggregation container, ECS Fargate for aggregation jobs, DynamoDB, S3, SSM, and CloudWatch.

---

## Prerequisites

### 1. AWS Account and CLI

You need an AWS account with programmatic access. Install and configure the CLI:

```bash
# Install AWS CLI v2 (Linux)
curl "https://awscli.amazonaws.com/awscli-exe-linux-x86_64.zip" -o "awscliv2.zip"
unzip awscliv2.zip && sudo ./aws/install

# Verify
aws --version   # should show aws-cli/2.x.x

# Configure credentials
aws configure
# Enter: AWS Access Key ID, Secret Access Key, default region (us-east-1), output (json)
```

The IAM user or role you configure needs these permissions:
- `AWSLambda_FullAccess`
- `AmazonDynamoDBFullAccess`
- `AmazonS3FullAccess`
- `AmazonECS_FullAccess`
- `AmazonECRFullAccess`
- `AmazonAPIGatewayAdministrator`
- `AWSCloudFormationFullAccess`
- `AmazonSSMFullAccess`
- `CloudWatchFullAccess`
- `IAMFullAccess` (needed for SAM to create Lambda execution roles)

### 2. AWS SAM CLI

```bash
# Linux x86_64
curl -L https://github.com/aws/aws-sam-cli/releases/latest/download/aws-sam-cli-linux-x86_64.zip \
  -o sam-cli.zip
unzip sam-cli.zip -d sam-installation
sudo ./sam-installation/install

# Verify
sam --version   # should show SAM CLI, version 1.x.x
```

### 3. Docker

Docker must be running to build the aggregation container image.
Verify: `docker info`

### 4. Python dependencies (for seed scripts)

```bash
cd /home/blaze/vs-code/federated_learning_model/coordinator
pip install boto3 cryptography numpy
```

---

## Step 1 — Generate the Ed25519 Signing Key

The aggregator signs every new global model. The key lives in SSM Parameter Store.
Run this once before deploying:

```bash
cd /home/blaze/vs-code/federated_learning_model/coordinator
python scripts/generate_signing_key.py
```

This writes two SSM parameters:
- `/fl-coordinator/ed25519-private-key` (SecureString) — used by the aggregation container
- `/fl-coordinator/ed25519-public-key` (String) — embed in daemon `config.toml`

Verify in AWS Console → Systems Manager → Parameter Store, or:

```bash
aws ssm get-parameter --name /fl-coordinator/ed25519-public-key --region us-east-1
```

**Keep the private key in SSM only. Never commit it to git.**

---

## Step 2 — Build and Push the Aggregation Docker Image to ECR

The aggregation worker runs as an ECS Fargate task. The `template.yaml` references
`{account_id}.dkr.ecr.{region}.amazonaws.com/fl-aggregation-worker:latest`.
You must push the image before deploying the SAM stack, otherwise the task definition
will fail to pull on first launch.

```bash
export AWS_ACCOUNT_ID=$(aws sts get-caller-identity --query Account --output text)
export AWS_REGION=us-east-1

# 1. Create the ECR repository (one-time)
aws ecr create-repository \
  --repository-name fl-aggregation-worker \
  --region $AWS_REGION

# 2. Authenticate Docker to ECR (token is valid for 12 hours)
aws ecr get-login-password --region $AWS_REGION \
  | docker login --username AWS --password-stdin \
    $AWS_ACCOUNT_ID.dkr.ecr.$AWS_REGION.amazonaws.com

# 3. Build the image
cd /home/blaze/vs-code/federated_learning_model/coordinator/aggregation
docker build -t fl-aggregation-worker .

# 4. Tag for ECR
docker tag fl-aggregation-worker:latest \
  $AWS_ACCOUNT_ID.dkr.ecr.$AWS_REGION.amazonaws.com/fl-aggregation-worker:latest

# 5. Push
docker push $AWS_ACCOUNT_ID.dkr.ecr.$AWS_REGION.amazonaws.com/fl-aggregation-worker:latest
```

Verify the push succeeded:
```bash
aws ecr list-images --repository-name fl-aggregation-worker --region $AWS_REGION
```

The Fargate task definition in `template.yaml` already references:
```
{AWS::AccountId}.dkr.ecr.{AWS::Region}.amazonaws.com/fl-aggregation-worker:latest
```
CloudFormation substitutes those at deploy time, so no manual edits needed.

---

## Step 3 — Deploy the SAM Stack

The SAM template (`coordinator/template.yaml`) provisions all AWS resources:
4 DynamoDB tables, 2 S3 buckets, API Gateway HTTP API, 7 Lambda functions,
ECS Fargate cluster + task definition, IAM roles, and CloudWatch log groups.

```bash
cd /home/blaze/vs-code/federated_learning_model/coordinator

# Build Lambda deployment packages
sam build

# Deploy (interactive first time — saves answers to samconfig.toml)
sam deploy --guided
```

During `sam deploy --guided` you will be prompted for:

| Prompt                           | Value to enter                        |
|----------------------------------|---------------------------------------|
| Stack Name                       | `fl-coordinator`                      |
| AWS Region                       | `us-east-1` (or your region)          |
| Parameter Environment            | `dev`                                 |
| Confirm changeset                | `y`                                   |
| Allow SAM CLI IAM role creation  | `y`                                   |
| Save to samconfig.toml           | `y`                                   |

After the first deploy, subsequent deploys are one command:
```bash
sam build && sam deploy
```

At the end of `sam deploy`, outputs are printed to the terminal. Save these:

```
ApiEndpoint      = https://<api-id>.execute-api.us-east-1.amazonaws.com
IngestionBucketName = fl-ingestion-<account-id>-dev
EpochTableName   = FederatedEpochTable
...
```

Your Lambda functions are now live at the `ApiEndpoint` URL.

---

## Step 4 — Set Up mTLS (Custom Domain + CA Bundle)

mTLS requires a custom domain name. The default `execute-api` endpoint does not support
client certificate validation.

### 4a. Generate your root CA

```bash
cd /home/blaze/vs-code/federated_learning_model/coordinator
bash scripts/generate_ca.sh
```

This produces:
- `ca.key` — **keep offline and secure, never upload this**
- `ca.pem` — the CA certificate, uploaded to S3 as the mTLS truststore

### 4b. Upload the CA bundle to S3

```bash
export CA_BUNDLE_BUCKET=fl-ca-bundle-${AWS_ACCOUNT_ID}-dev

aws s3 cp ca.pem s3://$CA_BUNDLE_BUCKET/ca-bundle.pem \
  --region $AWS_REGION
```

Or use the provided script:
```bash
bash scripts/upload_ca_bundle.sh
```

### 4c. Request an ACM certificate for your custom domain

API Gateway requires an ACM certificate in the **same region** as your API.

```bash
# Replace with your actual domain
export DOMAIN=coordinator.fl-platform.example.com

aws acm request-certificate \
  --domain-name $DOMAIN \
  --validation-method DNS \
  --region $AWS_REGION
```

Follow the DNS validation steps shown in the ACM console (add the CNAME record to
your DNS provider). The certificate status must be `ISSUED` before continuing.

### 4d. Create the custom domain in API Gateway

```bash
# Get your ACM certificate ARN
CERT_ARN=$(aws acm list-certificates --region $AWS_REGION \
  --query "CertificateSummaryList[?DomainName=='$DOMAIN'].CertificateArn" \
  --output text)

# Create the custom domain with mTLS enabled
aws apigatewayv2 create-domain-name \
  --domain-name $DOMAIN \
  --domain-name-configurations \
    "CertificateArn=$CERT_ARN,EndpointType=REGIONAL" \
  --mutual-tls-authentication \
    "TruststoreUri=s3://$CA_BUNDLE_BUCKET/ca-bundle.pem" \
  --region $AWS_REGION
```

Note the `ApiGatewayDomainName` in the output — create a CNAME DNS record pointing
your domain to it.

### 4e. Map the custom domain to your API

```bash
# Get your API ID from the SAM stack output
API_ID=$(aws cloudformation describe-stacks \
  --stack-name fl-coordinator \
  --query "Stacks[0].Outputs[?OutputKey=='ApiEndpoint'].OutputValue" \
  --output text | sed 's|https://||' | cut -d. -f1)

aws apigatewayv2 create-api-mapping \
  --domain-name $DOMAIN \
  --api-id $API_ID \
  --stage '$default' \
  --region $AWS_REGION
```

After DNS propagates, your coordinator is live at:
`https://coordinator.fl-platform.example.com`

### 4f. Issue certificates for each organization

For each participating organization:
```bash
bash scripts/issue_org_cert.sh --org-id org-acme-bank
```

This produces:
- `org-acme-bank.key` — send to the organization; they load it into their TPM/HSM
- `org-acme-bank.pem` — send to the organization; path goes in their `cert_path` config

The organization puts these in their daemon config:
```toml
[certificates]
cert_path = "/etc/fl-daemon/certs/org-acme-bank.pem"
ca_bundle_path = "/etc/fl-daemon/certs/fl-platform-ca.pem"
[certificates.key_storage]
type = "tpm"
device_path = "/dev/tpmrm0"
```

---

## Step 5 — Register Organizations in OrgTable

Each organization must have a record in `FederatedOrgTable` before their daemon can connect.

```bash
cd /home/blaze/vs-code/federated_learning_model/coordinator

# Register org (run once per organization)
python scripts/register_org.py \
  --org-id org-hospital-a \
  --display-name "Hospital A"

python scripts/register_org.py \
  --org-id org-hospital-b \
  --display-name "Hospital B"
```

The `--org-id` value must exactly match the `CN` field in the organization's mTLS certificate.
For example, if you ran `issue_org_cert.sh --org-id org-hospital-a`, the CN is `org-hospital-a`.

Verify:
```bash
aws dynamodb get-item \
  --table-name FederatedOrgTable \
  --key '{"org_id":{"S":"org-hospital-a"}}' \
  --region $AWS_REGION
```

---

## Step 6 — Seed the Initial Epoch

```bash
cd /home/blaze/vs-code/federated_learning_model/coordinator

# 1. Create the epoch as PENDING
python scripts/seed_epoch.py \
  --model-id fraud-detection-v2 \
  --epoch-number 1 \
  --threshold 2

# 2. Activate it (writes atomic Lock Item — single-active-epoch enforcement)
python scripts/activate_epoch.py \
  --epoch-id "EPOCH#fraud-detection-v2#1"
```

`seed_epoch.py` does the following:
- Creates a 100-element float32 zeros array as the initial model
- Uploads it to S3: `s3://fl-ingestion-<account>-dev/models/fraud-detection-v2/v1/model.npy`
- Signs the model bytes with the Ed25519 key from SSM
- Writes a PENDING epoch record to `FederatedEpochTable`

`activate_epoch.py` does the following:
- Fetches the epoch to confirm it is PENDING
- Writes a Lock Item `PK=MODEL#fraud-detection-v2#LOCK` with `attribute_not_exists` condition
  (prevents concurrent activations — TOCTOU fix)
- Sets epoch status to ACTIVE

After this, daemon polling for `model_id=fraud-detection-v2` will return EpochMetadata.

Verify:
```bash
aws dynamodb get-item \
  --table-name FederatedEpochTable \
  --key '{"epoch_id":{"S":"EPOCH#fraud-detection-v2#1"}}' \
  --query 'Item.status.S' \
  --region $AWS_REGION
# Expected: "ACTIVE"
```

---

## Step 7 — Configure the Daemon for Production

Update your daemon's `config.toml` to point at the live coordinator:

```toml
organization_id = "org-hospital-a"   # must match CN in your mTLS certificate

[coordinator]
base_url = "https://coordinator.fl-platform.example.com"
poll_interval_secs = 60
max_backoff_secs = 300
request_timeout_secs = 30
max_retries = 5

[certificates]
cert_path = "/etc/fl-daemon/certs/org-hospital-a.pem"
ca_bundle_path = "/etc/fl-daemon/certs/fl-platform-ca.pem"
cert_dir = "/etc/fl-daemon/certs"
rotation_warning_days = 30
check_interval_secs = 3600
[certificates.key_storage]
type = "tpm"
device_path = "/dev/tpmrm0"

# ... (rest of training, privacy, resources config same as dev)

[[models]]
model_id = "fraud-detection-v2"
priority = 1
data_source = "/data/training/fraud_data.parquet"
schema_path = "/etc/fl-daemon/fraud_detection.schema.json"
```

Important notes:
- `type = "tpm"` and `framework = "pytorch"` must be **lowercase** (Rust serde requirement)
- `ca_bundle_path` must point to the `ca.pem` you generated in Step 4a
- The daemon communicates outbound-only over HTTPS port 443 — no inbound firewall changes needed

Run the daemon:
```bash
RUST_LOG=info ./target/release/fl-client-daemon /etc/fl-daemon/config.toml
```

---

## Step 8 — Smoke Test the Live API

Use curl to verify all endpoints before connecting a daemon:

```bash
export BASE=https://coordinator.fl-platform.example.com

# Health (no auth required)
curl -s $BASE/api/health

# Epoch query (mTLS required — pass cert + key + CA bundle)
curl -s --cert org-hospital-a.pem \
       --key org-hospital-a.key \
       --cacert ca.pem \
       "$BASE/api/epochs/active?model_id=fraud-detection-v2" | python3 -m json.tool

# Model download URL
curl -s --cert org-hospital-a.pem --key org-hospital-a.key --cacert ca.pem \
  -X POST $BASE/api/models/download-url \
  -H "Content-Type: application/json" \
  -d '{"model_id":"fraud-detection-v2","model_version":"v1"}' | python3 -m json.tool
```

If you get HTTP 403 on the epoch query, the most common causes are:
1. The `org_id` in the cert CN doesn't match the OrgTable record — recheck Step 5
2. The CA bundle wasn't uploaded correctly — recheck Step 4b
3. API Gateway custom domain not yet mapped — recheck Step 4e

---

## Step 9 — Monitor with CloudWatch

All Lambda invocations, aggregation jobs, and API access are logged automatically.

```bash
# Tail all coordinator Lambda logs
sam logs --stack-name fl-coordinator --tail

# Watch a specific function
sam logs --stack-name fl-coordinator \
  --name AggregationTriggerFunction --tail

# View API access logs
aws logs tail /fl-coordinator/api-access --follow --region $AWS_REGION

# View aggregation container logs
aws logs tail /fl-coordinator/aggregation --follow --region $AWS_REGION
```

Key metrics to watch in CloudWatch → Metrics → FederatedLearning namespace:
- `ACTIVE_EPOCHS` — should be 1 per model during a round
- `SUBMISSIONS_RECEIVED` — count per epoch
- `AGGREGATION_DURATION_SECONDS` — Fargate task runtime
- `AGGREGATION_UPDATES_USED` — how many updates Multi-Krum selected
- `MODEL_PUBLISH_FAILURE` — should stay at 0; alert if non-zero

---

## Step 10 — What Happens During a Training Round on AWS

This is the full flow once everything is deployed:

1. **Daemon polls** `GET /api/epochs/active?model_id=fraud-detection-v2`
   - API Gateway validates the mTLS client cert against the CA bundle in S3
   - Injects `CN=org-hospital-a` into the request context
   - Routes to `EpochQueryFunction` Lambda
   - Lambda queries `FederatedEpochTable` GSI `model_id-status-index` for ACTIVE epoch
   - Returns `EpochMetadata` JSON including model hash, signature, participant list

2. **Daemon downloads model** via pre-signed S3 URL from `POST /api/models/download-url`
   - Lambda calls `s3.generate_presigned_url()` for `models/fraud-detection-v2/v1/model.npy`
   - URL is valid for 15 minutes
   - Daemon downloads directly from S3 (bypasses Lambda)

3. **Daemon trains locally**, applies differential privacy + secure aggregation masking

4. **Daemon uploads update** via pre-signed PUT URL from `POST /api/updates/upload-url`
   - S3 key is deterministic: `updates/fraud-detection-v2/1/org-hospital-a/update.bin`
   - URL valid for 30 minutes
   - Daemon uploads binary directly to S3

5. **Daemon submits completion** `POST /api/updates/complete`
   - Lambda validates `update_hash` (64-char hex), checks for duplicate submission
   - Writes to `FederatedSubmissionTable` — this triggers DynamoDB Streams

6. **DynamoDB Streams fires** `AggregationTriggerFunction` Lambda
   - Lambda counts submissions for the epoch
   - If count >= `secure_agg_threshold` (2 in the seed): atomically sets epoch `ACTIVE → AGGREGATING`
   - Calls `ecs.run_task()` on `FederatedLearningCluster`

7. **Fargate AggregationTask starts**
   - Downloads all update binaries from S3 using keys stored in `FederatedSubmissionTable`
   - Runs Multi-Krum (selects `n-f-2` updates, falls back to FedAvg if n < 2f+3)
   - Signs aggregate model with Ed25519 key from SSM
   - Uploads new model to S3: `models/fraud-detection-v2/v2/model.npy`
   - Sets epoch 1 → COMPLETED, creates epoch 2 as PENDING
   - Deletes Lock Item so epoch 2 can be activated

8. **Operator activates epoch 2** (or automate with a Lambda/Step Functions trigger):
   ```bash
   python scripts/activate_epoch.py --epoch-id "EPOCH#fraud-detection-v2#2"
   ```

---

## Tear-Down

To delete all AWS resources and avoid charges:

```bash
# Delete the SAM stack (removes Lambda, DynamoDB, S3, API Gateway, ECS, IAM roles)
aws cloudformation delete-stack --stack-name fl-coordinator --region $AWS_REGION

# Delete ECR repository
aws ecr delete-repository \
  --repository-name fl-aggregation-worker \
  --force --region $AWS_REGION

# Delete SSM parameters
aws ssm delete-parameter --name /fl-coordinator/ed25519-private-key --region $AWS_REGION
aws ssm delete-parameter --name /fl-coordinator/ed25519-public-key --region $AWS_REGION

# Delete ACM certificate (if created)
aws acm delete-certificate --certificate-arn $CERT_ARN --region $AWS_REGION
```

> **Note:** S3 buckets with objects cannot be deleted by CloudFormation automatically.
> If the stack deletion fails on the buckets, empty them first:
> ```bash
> aws s3 rm s3://fl-ingestion-<account-id>-dev --recursive
> aws s3 rm s3://fl-ca-bundle-<account-id>-dev --recursive
> ```
> Then retry `aws cloudformation delete-stack`.

---

## Resource Summary

| Resource                        | Name / Pattern                              | Cost driver             |
|---------------------------------|---------------------------------------------|-------------------------|
| S3 IngestionBucket              | `fl-ingestion-<account>-<env>`              | Storage + requests      |
| S3 CaBundleBucket               | `fl-ca-bundle-<account>-<env>`              | Minimal (static file)   |
| DynamoDB EpochTable             | `FederatedEpochTable`                        | PAY_PER_REQUEST          |
| DynamoDB SubmissionTable        | `FederatedSubmissionTable` (Streams ON)      | PAY_PER_REQUEST          |
| DynamoDB AuditTable             | `FederatedAuditTable`                        | PAY_PER_REQUEST          |
| DynamoDB OrgTable               | `FederatedOrgTable`                          | PAY_PER_REQUEST          |
| API Gateway HTTP API v2         | `fl-coordinator` stack                      | $1/million requests     |
| Lambda (7 functions)            | arm64, 128–256 MB                           | Free tier covers demo   |
| ECS Fargate cluster             | `FederatedLearningCluster`                  | Only runs during aggr.  |
| ECR repository                  | `fl-aggregation-worker`                     | $0.10/GB/month          |
| SSM Parameter Store             | `/fl-coordinator/*` (Standard tier)         | Free                    |
| CloudWatch Log Groups           | 30-day retention                            | ~$0.50/month            |

Projected cost for a hackathon demo (5–10 orgs, 50 rounds): **under $5** on $100 credit.
Fargate runs ~10 min per aggregation at 2 vCPU/4 GB = ~$0.01 per round.

---

## Common Issues

| Problem | Likely cause | Fix |
|---------|-------------|-----|
| `403 Forbidden` on all endpoints | org_id not in OrgTable, or CN mismatch | Re-run `register_org.py` with exact CN |
| `404 No active epoch` | Epoch not activated | Run `activate_epoch.py` |
| Aggregation task never starts | Threshold not met, or DynamoDB Streams not enabled | Check SubmissionTable stream is enabled in template.yaml |
| `NoSuchKey` in aggregation logs | S3 key mismatch between upload-url and update_complete | Both must use deterministic key `updates/{model_id}/{epoch}/{org}/update.bin` |
| `ConditionalCheckFailed` on activate | Another epoch already ACTIVE for the model | Complete or fail existing epoch first |
| Fargate task fails to pull image | ECR image not pushed, or region mismatch | Re-run Step 2 with correct `$AWS_REGION` |
| SAM deploy fails on ECR image | Image not pushed before template deploy | Push image (Step 2) before `sam deploy` |
| mTLS cert rejected by API Gateway | CA bundle not uploaded or wrong bucket | Re-check Step 4b; verify bucket name matches template |
