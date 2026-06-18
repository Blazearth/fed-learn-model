.PHONY: build up down logs test seed shell-dynamodb shell-localstack

# ── Local dev (Path B — no AWS needed) ───────────────────────────────────────
up:
	docker compose up -d --build
	@echo "\nCoordinator running at http://localhost:8082"
	@echo "DynamoDB Local:         http://localhost:8000"
	@echo "LocalStack S3:          http://localhost:4566\n"

down:
	docker compose down -v

logs:
	docker compose logs -f coordinator

rebuild:
	docker compose up -d --build --force-recreate coordinator

# ── Tests ─────────────────────────────────────────────────────────────────────
test:
	cd tests && pip install -q -r requirements.txt && pytest -v

test-unit:
	pip install -q boto3 moto pytest cryptography python-ulid
	pytest lambdas/ aggregation/tests/ -v

# ── Seed data against local stack ─────────────────────────────────────────────
seed: up
	@echo "Waiting for coordinator health..."
	@until curl -sf http://localhost:8082/api/health; do sleep 1; done
	python scripts/generate_signing_key.py --local
	python scripts/register_org.py --org-id org-hospital-a --display-name "Hospital A" --local
	python scripts/register_org.py --org-id org-hospital-b --display-name "Hospital B" --local
	python scripts/seed_epoch.py --model-id fraud-detection-v2 --epoch-number 1 --threshold 2 --local
	python scripts/activate_epoch.py --epoch-id "EPOCH#fraud-detection-v2#1" --local

# ── Helpers ───────────────────────────────────────────────────────────────────
shell-dynamodb:
	docker exec -it fl_dynamodb /bin/bash

list-tables:
	AWS_ACCESS_KEY_ID=test AWS_SECRET_ACCESS_KEY=test \
	aws dynamodb list-tables --endpoint-url http://localhost:8000 --region us-east-1

list-epochs:
	AWS_ACCESS_KEY_ID=test AWS_SECRET_ACCESS_KEY=test \
	aws dynamodb scan --table-name FederatedEpochTable \
	  --endpoint-url http://localhost:8000 --region us-east-1

# ── AWS deploy (Path A — when you have AWS credentials) ───────────────────────
sam-build:
	sam build

sam-deploy:
	sam deploy --guided

sam-logs:
	sam logs --stack-name fl-coordinator --tail
