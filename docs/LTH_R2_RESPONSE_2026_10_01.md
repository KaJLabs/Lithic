# LTH-R2 response — disabled Lithic/LithoVM candidate

Autha's formal R2 report is `Autha_Lithic_LithoVM_Retest_LTH-R2_2026-10-01.docx`,
SHA-256 `e4fc08b0e07b1fd34c2c81b4999e7cb03f2a97520645f5a62753c9268a377095`.
It assesses the original 7d8bcc7 source ZIP, not subsequent changes. The
candidate remains disabled. No finding below is claimed closed by this note;
Autha's focused R3 retest is required.

## Engineering response

- LTH-10: Retain standard EVM failure semantics. `ErrOutOfGas` and other
  non-REVERT errors consume the enclosing EVM call's remaining gas, even if
  the native work envelope is 10M and the transaction frame is 30M. Turning
  a native OOG into `ErrExecutionReverted` solely to preserve gas would
  misclassify failure and change EVM interoperability. The previous comment
  promised too much; it now distinguishes successful calls from failed calls.
  Regressions cover the direct meter's 30M frame and EVM.Call's 30M failure
  handling. Foundation and Autha should confirm this policy disposition.
- LTH-11: Restore the release-patched SDK variable assignment before module
  transaction commands are built. Test a module's local `--gas-adjustment`
  default, which shadows the root persistent flag.
- LTH-12 and LTH-07: The next review package includes the SHA-256-pinned
  Cosmos SDK compatibility patch and explicit apply step. Packaging disables
  `core.autocrlf` so archived files are raw Git blob bytes. A deterministic
  package test verifies byte identity with `git show` while the source repo
  has `core.autocrlf=true`. Full build and auditor verification remain open.

## Outstanding decisions and evidence

LTH-03 remains Foundation policy: the 20,000-gas growth unit and 10M native
cap are provisional. Under the current hex-in-JSON record layout, about 7.3
KiB of code is deployable under that cap, while gateway admission allows up
to 64 KiB. Do not treat admission as deployability. The approved synthetic
full-block benchmark can inform, but cannot itself approve, gas economics.

LTH-06 requires a single new source pin and a complete evidence rerun on the
pinned Go and patched SDK. LTH-08 remains a next-release gate for the full
EVM keeper suite. LTH-09 needs a Foundation-approved coordinated store
cutover and networked rollback rehearsal before any Makalu registration.
The router salt namespace, literal `initialize` guard, and frontend address
prediction must be documented and tested before production use. LEP100-15,
Makalu end-to-end deployment/recovery, dedicated compiler/VM review, and a
signed reproducible release remain separate gates. No LAX, production, or
MultX deployment or activation is authorized.
