# ABI Specification

## Purpose
Defines how Lithic contracts expose callable interfaces, errors, capabilities, mutability, and async fulfillment expectations.

## Required fields per function

- `name`
- `selector`
- `inputs`
- `outputs`
- `mutability`
- `payable`
- `async`
- `required_capabilities`
- `errors`

## Example

```json
{
  "name": "invoke_model",
  "selector": "0x4f8a11b2",
  "inputs": [
    {"name": "provider_id", "type": "bytes32"},
    {"name": "req", "type": "AIRequest"}
  ],
  "outputs": [
    {"name": "pending", "type": "PendingReceipt"}
  ],
  "mutability": "nonpayable",
  "payable": false,
  "async": true,
  "required_capabilities": ["AIInvoke", "Spend"],
  "errors": ["BudgetExceeded", "UnknownProvider", "InvalidRequest"]
}
```
