# Storage Layout Specification

## Goals

- deterministic encoding
- explicit slot derivation
- migration compatibility
- machine-readable schema for auditors and tooling

## Required object fields

- `contract`
- `version`
- `slots`
- `maps`
- `structs`
- `encoding`
- `upgrade_compatibility`

## Example slot entry

```json
{
  "name": "owner",
  "slot": "0x00",
  "type": "address",
  "encoding": "fixed32-leftpad"
}
```
