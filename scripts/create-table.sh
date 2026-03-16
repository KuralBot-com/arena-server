#!/bin/bash
# Creates the KuralBot DynamoDB table with all GSIs.
# Usage: ./scripts/create-table.sh [endpoint-url]
# For local dev: ./scripts/create-table.sh http://localhost:8000

ENDPOINT=""
if [ -n "$1" ]; then
  ENDPOINT="--endpoint-url $1"
fi

TABLE_NAME="${DYNAMODB_TABLE:-KuralBot}"

aws dynamodb create-table $ENDPOINT \
  --table-name "$TABLE_NAME" \
  --attribute-definitions \
    AttributeName=pk,AttributeType=S \
    AttributeName=sk,AttributeType=S \
    AttributeName=gsi1pk,AttributeType=S \
    AttributeName=gsi1sk,AttributeType=S \
    AttributeName=gsi2pk,AttributeType=S \
    AttributeName=gsi2sk,AttributeType=S \
    AttributeName=gsi3pk,AttributeType=S \
    AttributeName=gsi3sk,AttributeType=S \
    AttributeName=gsi4pk,AttributeType=S \
    AttributeName=gsi4sk,AttributeType=S \
    AttributeName=gsi5pk,AttributeType=S \
    AttributeName=gsi5sk,AttributeType=S \
    AttributeName=gsi6pk,AttributeType=S \
    AttributeName=gsi6sk,AttributeType=S \
    AttributeName=gsi7pk,AttributeType=S \
    AttributeName=gsi7sk,AttributeType=S \
  --key-schema \
    AttributeName=pk,KeyType=HASH \
    AttributeName=sk,KeyType=RANGE \
  --global-secondary-indexes \
    '[
      {
        "IndexName": "GSI1",
        "KeySchema": [
          {"AttributeName": "gsi1pk", "KeyType": "HASH"},
          {"AttributeName": "gsi1sk", "KeyType": "RANGE"}
        ],
        "Projection": {"ProjectionType": "ALL"}
      },
      {
        "IndexName": "GSI2",
        "KeySchema": [
          {"AttributeName": "gsi2pk", "KeyType": "HASH"},
          {"AttributeName": "gsi2sk", "KeyType": "RANGE"}
        ],
        "Projection": {"ProjectionType": "ALL"}
      },
      {
        "IndexName": "GSI3",
        "KeySchema": [
          {"AttributeName": "gsi3pk", "KeyType": "HASH"},
          {"AttributeName": "gsi3sk", "KeyType": "RANGE"}
        ],
        "Projection": {"ProjectionType": "ALL"}
      },
      {
        "IndexName": "GSI4",
        "KeySchema": [
          {"AttributeName": "gsi4pk", "KeyType": "HASH"},
          {"AttributeName": "gsi4sk", "KeyType": "RANGE"}
        ],
        "Projection": {"ProjectionType": "ALL"}
      },
      {
        "IndexName": "GSI5",
        "KeySchema": [
          {"AttributeName": "gsi5pk", "KeyType": "HASH"},
          {"AttributeName": "gsi5sk", "KeyType": "RANGE"}
        ],
        "Projection": {"ProjectionType": "ALL"}
      },
      {
        "IndexName": "GSI6",
        "KeySchema": [
          {"AttributeName": "gsi6pk", "KeyType": "HASH"},
          {"AttributeName": "gsi6sk", "KeyType": "RANGE"}
        ],
        "Projection": {"ProjectionType": "ALL"}
      },
      {
        "IndexName": "GSI7",
        "KeySchema": [
          {"AttributeName": "gsi7pk", "KeyType": "HASH"},
          {"AttributeName": "gsi7sk", "KeyType": "RANGE"}
        ],
        "Projection": {"ProjectionType": "ALL"}
      }
    ]' \
  --billing-mode PAY_PER_REQUEST

echo "Table '$TABLE_NAME' created successfully."
