# Manifest Specification

## Purpose
Canonical package metadata for compilation target, dependencies, upgrade policy, capabilities, and deployment profile.

## Required sections

- `package`
- `contract`
- `capabilities`
- `dependencies`
- `target`

## Example

```toml
[package]
name = "treasury-agent"
version = "1.0.0"
edition = "2026"

[contract]
kind = "service"
entry = "contracts/examples/treasury_agent.lithic"
upgrade_policy = "governed"

[capabilities]
required = ["Admin", "AIInvoke", "Spend", "EmitReceipt"]

[dependencies]
lscl = ">=1.0.0"
lep100-ft = ">=1.0.0"
budget = ">=1.0.0"
receipts = ">=1.0.0"

[target.makalu]
vm = "lithovm-v1"
chain_id = 700777
```
