# Lithic/LithoVM fuzz smoke evidence

Date: 2026-09-26

Environment: WSL Linux x86_64, `rustc 1.100.0-nightly
(5ceaf6608 2026-09-25)`, `cargo-fuzz 0.13.2`, AddressSanitizer defaults.

Targets:

- `bytecode_decode`: arbitrary bytes through strict decode; accepted programs
  must encode/decode canonically.
- `compiler_runtime`: bounded arbitrary UTF-8 through compilation;
  accepted programs must compile deterministically and load in LithoVM.
- `stateful_maps`: bounded credit/debit sequences checked against an independent
  balance model; failed debits must preserve the complete storage snapshot.

Commands:

```sh
cargo +nightly fuzz run bytecode_decode --fuzz-dir fuzz -- \
  -max_total_time=15 -max_len=65536 -timeout=10 -print_final_stats=1
cargo +nightly fuzz run compiler_runtime --fuzz-dir fuzz -- \
  -max_total_time=15 -max_len=65536 -timeout=10 -print_final_stats=1
cargo +nightly fuzz run stateful_maps --fuzz-dir fuzz -- \
  -max_total_time=15 -max_len=640 -timeout=10 -print_final_stats=1
```

Results:

| Target | Executions | Runtime | Peak RSS | Crash/hang |
| --- | ---: | ---: | ---: | --- |
| bytecode_decode | 5,318,725 | 16 seconds | 506 MB | None |
| compiler_runtime | 41,772 | 17 seconds | 404 MB | None |
| stateful_maps | 22,978 | 16 seconds | 467 MB | None |

The generated local smoke corpus was not promoted wholesale; the reviewed
failure/effect seeds and a reviewed stateful-map seed remain committed. These bounded runs establish harness
operation, not production security acceptance. A release candidate still
requires longer retained-corpus campaigns, review of coverage, regression
promotion for any finding and independent security review.
