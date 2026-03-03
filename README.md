# Lithic — AI-Native Smart Contract Language for Lithosphere

[![Status](https://img.shields.io/badge/status-draft-blue.svg)](#)
[![Language](https://img.shields.io/badge/language-Lithic%20(.lithic)-111827.svg)](#)
[![Target](https://img.shields.io/badge/target-LithoVM-0ea5e9.svg)](#)
[![Standards](https://img.shields.io/badge/standards-LEP100%20(1%E2%80%9314)-7b2cff.svg)](#lep100-standards)
[![Security](https://img.shields.io/badge/security-capability--based-22c55e.svg)](#security-model)
[![AI](https://img.shields.io/badge/ai-typed%20services%20%2B%20async-f97316.svg)](#ai-native-primitives)
[![License](https://img.shields.io/badge/license-Apache--2.0-black.svg)](#license)

**Website:** `lithic.at` • `lithiclang.ai`  
**Docs:** `docs.lithosphere.network`  
**Proposed by:** J. King Kasr  
**Maintained by:** KaJ Labs  
**Network:** Lithosphere  
**VM Target:** LithoVM  

---

## What is Lithic?

**Lithic** is Lithosphere’s native smart-contract programming language for deploying **AI-native** applications onchain.

It is designed for:

- **Typed AI services** and **async request/fulfill** workflows
- Deterministic, consensus-safe execution targeting **LithoVM**
- **Cost caps**, **per-user budgets**, and programmable spend governance
- **Capability-based permissions** for safe system programming
- Cryptographic **provenance receipts** and audit-ready trace records
- Optional **zk-verifiable AI execution** for high-assurance workflows
- Secure, audited reference modules via **LSCL** (Lithosphere Secure Contracts Library)

---

## Key Features

### AI-native primitives
- `ai.service` declarations with typed configuration
- `ai.request` / `ai.fulfill` lifecycle
- Async callbacks + `.timeout()` handling
- Receipt binding (model/input/output hashes) for verification

### Built-in economic safety
- Per-call max cost ceilings
- Per-user budgets and deterministic epochs
- Escrow locking + settlement patterns

### Capability-based security
- Explicit permissions for sensitive ops (AI calls, transfers, syscalls, zk verification)
- Principle-of-least-privilege by construction

### Production-grade tooling
- `lithc` compiler (`.lithic` → LithoVM bytecode)
- `lithfmt` formatter + `lithlint` security lints
- LSP support (editor integrations)
- Deterministic devnet + fuzz harnesses + conformance tests

### LSCL secure modules
OpenZeppelin-like primitives for AI and assets:
- `AgentWallet` • `ToolRouter` • `PolicyGuard`
- `BudgetGuard` • `TraceRecorder`
- NFT / Multi-token / Royalties / Metadata / Bridge interfaces

---

## Quick Example (AI + Async + Timeout)

```lithic
requires AI_CALL

contract RiskAnalyzer {
  state { last_report: string }

  ai.service GPT4 {
    endpoint: "agii://provider42/gpt4"
    max_cost: 10 LITHO
    // zk_required: true  // optional (LEP100-5)
  }

  public fn analyze(text: string) {
    let req = ai.request GPT4 {
      prompt: text,
      temperature: 0.1,
      max_tokens: 400
    }

    ai.fulfill(req) |response| {
      self.last_report = response.text
    }
    .timeout(20 blocks)
    .on_timeout { revert("AI timeout") }
  }
}


LEP100 Standards
Lithic is the reference language target for the LEP100 modular protocol stack:
LEP100-1 — Lithic Core Specification
LEP100-2..5 — AI providers, budgets, receipts, zk execution
LEP100-6..13 — NFTs, composability, shared ownership, multi-token, royalties, metadata, marketplace hooks, bridge mint/burn
LEP100-14 — Privacy-Preserving Account Linking (PPAL)

Toolchain
Compiler
lithc — compile, test, deploy
Developer Experience
lithfmt — canonical formatting
lithlint — security checks (missing caps, unsafe async, budget misconfig)
lithic-lsp — language server for IDEs
Testing & Security
Deterministic devnet runner
AI mock provider + receipt verifier
Fuzz testing harnesses
LEP100 conformance test suite (1–14)

Security Model
Lithic enforces capability-based permissions:
requires AI_CALL
requires TOKEN_TRANSFER
requires ZK_VERIFY
requires BRIDGE_MINT
requires PPAL_CONSUME

Contracts MUST explicitly declare sensitive privileges, enabling:
safer audits
safer deployment policies
less accidental attack surface

Repository Layout (recommended)
lithic/
├─ compiler/            # lithc (frontend + IR + codegen)
├─ stdlib/              # standard library modules
├─ lscl/                # secure reference modules (optional subtree/submodule)
├─ specs/               # LEP100-1 (and related) language specs
├─ examples/            # sample contracts (AI, NFTs, bridges)
├─ tests/               # compiler + runtime tests
└─ docs/                # documentation site content


Credits
Lithic is proposed by J. King Kasr and maintained by KaJ Labs for the Lithosphere ecosystem.

License
Apache-2.0 (recommended)

