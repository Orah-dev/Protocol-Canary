# Changelog

All notable changes to this project are documented in this file.

## [Unreleased]

- `stellar-canary lock` (CF-03): writes a `.stellar-canary.lock` at the
  project root pinning the active fixture pack's canonical
  `packDigest` (`canary-pack-v1`, CF-02, computed in
  `canary_fixtures::digest`), the target protocol, and the tool
  version, with a deterministic serializer (fixed key order, no
  timestamps — the same pack always yields byte-identical output). The
  whole pack is resolved and validated before the lockfile is touched,
  so an unreachable or invalid fixtures directory reports a clear error
  and never leaves a partial lockfile behind. Lockfile v1 parsing
  lives in `canary_config::lockfile`.
- `stellar-canary diff <current_report> <baseline_report>` (CF-05):
  compares two stored JSON reports offline and prints the comparison as
  `diffVersion: 1` JSON (`canary_report::diff`, rendered by
  `DiffJsonReporter`), with entries sorted deterministically by
  `(surface, testId)`. Exit codes follow CF-05's dedicated gating
  convention: `0` = no-change or improved, `2` = regression or
  coverage-lost (also `2` when the reports are not comparable —
  differing/unsupported `schemaVersion` or differing `targetProtocol`,
  with an explanatory diagnostic), `1` = report file unreadable or
  unparseable. This 0/1/2 mapping applies to `diff` only and is
  deliberately separate from the global `check` exit-code contract.
- `canary-core` no longer exports the unused `CompatibilityTest` trait and
  `CompatibilityPlanner` marker type; the `planner` module is removed and
  `engine` now exports only `ExecutionContext`. Surface runners implement
  their own execution traits, and fixture planning lives in
  `canary-runner`'s scheduler (`build_plan`).
- `RpcError::NetworkMismatch` and `RpcError::ProtocolMismatch` are now
  actively raised: `canary_rpc::validate_network_info` constructs them
  from a `getNetwork` response, and `check` compares the observed network
  identity against `--network`/`--protocol`. A passphrase mismatch aborts
  as a configuration error (exit 2); an observed-protocol mismatch prints
  a `warning:` line on stderr and the run continues (the report's
  `(observed protocol N)` annotation is unchanged).

## [0.1.1]

- `canary-xdr` now also supports the `"ContractExecutable"` XDR type name
  (previously only `"StellarValue"`), needed by `ProtocolCanary-Fixtures`
  to test CAP-0085's `CONTRACT_EXECUTABLE_EXTERNAL_REF` case.

## [0.1.0]

Initial release.

- `stellar-canary check`: config → project detection → fixture loading/
  validation → compatibility planner → XDR/RPC/Soroban runners → policy
  evaluation → terminal/JSON/Markdown report → documented exit code
  (0 pass, 1 compatibility failure, 2 configuration error, 3 execution/
  RPC error, 4 invalid fixture, 5 internal error).
- `stellar-canary inspect`, `stellar-canary fixtures`, `stellar-canary
  report`, `stellar-canary version`.
- XDR compatibility via the official `stellar-xdr` 28.0.0 crate
  (decode-success, decode-failure, roundtrip, encode-equals).
- A Stellar RPC client (`getNetwork`, `getLatestLedger`,
  `simulateTransaction`) with bounded retries and field-shape assertions.
- Soroban compatibility via unsigned `InvokeHostFunction` transaction
  construction and simulation — no private key is ever read, and no
  transaction is ever submitted.
- Three real Protocol 28 fixtures (one per surface) under
  `tests/fixtures/protocol-28`, verified against
  `soroban-testnet.stellar.org`; see `docs/protocol-28.md`.
- Git metadata (commit/branch/dirty status) and end-to-end test coverage
  for every documented exit code.

### Known gaps

- `canary_core::CacheStore` (a local, file-backed, per-fixture result
  cache keyed by fixture id/protocol/project fingerprint/RPC endpoint/
  observed protocol) is implemented and unit-tested, but not yet wired
  into `check`'s execution path — every run currently calls RPC/Soroban
  fresh. Not required by this project's own MVP definition, but worth
  doing before relying on this tool to avoid rate limits at scale.
