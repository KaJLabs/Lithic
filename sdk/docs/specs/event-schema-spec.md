# Event Schema Specification

## Goals

- canonical topic hashing
- indexable fields
- compatibility across indexers and bridges

## Standard event families

- Transfer
- Approval
- BudgetReserved
- BudgetSettled
- BudgetRefunded
- AIRequested
- AIFulfilled
- ReceiptVerified
- TimeoutTriggered
- BridgeMint
- BridgeBurn
- LinkCommitted
- LinkRevoked

## Example

```json
{
  "name": "BudgetReserved",
  "topic": "0x9d7c...",
  "fields": [
    {"name": "account", "type": "address", "indexed": true},
    {"name": "reservation_id", "type": "bytes32", "indexed": true},
    {"name": "amount", "type": "u128", "indexed": false}
  ]
}
```
