# `diff`

Compares two previously generated JSON reports and renders the
comparison as machine-readable `diffVersion: 1` JSON (CF-05). Nothing is
re-run and no network call is made — the diff is computed entirely from
the two stored files.

```text
Compare two stored JSON reports and render the diff (CF-05)

Usage: stellar-canary diff <CURRENT_REPORT> <BASELINE_REPORT>

Arguments:
  <CURRENT_REPORT>  Path to the current report (produced by `stellar-canary check --json`)
  <BASELINE_REPORT> Path to the baseline report to compare the current report against
```

Note the argument order: **current first, baseline second**. The diff
answers "what changed since the baseline", so `pass -> fail` (a
regression) is what you get when a test passed in the baseline and fails
now.

## Exit codes (CF-05)

`diff` uses its own dedicated CI-gating exit codes — deliberately
**not** the global [exit-code contract](../exit-codes.md) that `check`
and friends use:

| Code | Meaning |
|---|---|
| `0` | Verdict `no-change` or `improved` — safe to merge. |
| `2` | Verdict `regression` or `coverage-lost`; also used when the two reports are **not comparable** (see below). |
| `1` | Execution/syntax error: a report file could not be read or is not a valid report. |

## Comparison eligibility

Two reports are only comparable when they carry the **same, supported
`schemaVersion`** and the **same `targetProtocol`**. Comparing a
Protocol 28 report against a Protocol 29 baseline would otherwise
produce a diff full of cross-protocol noise that looks like a real
regression, so `diff` refuses instead: it prints a diagnostic
explaining exactly why (a `schemaVersion mismatch` or `protocol
mismatch`) on stderr, produces no JSON, and exits `2`.

## Verdicts

The `verdict` field follows a strict precedence: `regression` beats
`coverage-lost`, which beats `improved`, which beats `no-change`.

- **regression** — at least one test that ran in both reports got
  strictly worse (`pass` → `warning`/`fail`/`error`, etc.).
- **coverage-lost** — at least one baseline test is absent from the
  current report, and nothing regressed.
- **improved** — every difference is a status getting better; nothing
  regressed or vanished.
- **no-change** — nothing differs.

Newly added tests (present only in the current report) appear in the
`added` array but never change the verdict: no existing test got worse.

## Output shape

```json
{
  "diffVersion": 1,
  "verdict": "regression",
  "transitions": [
    { "surface": "rpc", "testId": "p28-rpc-1", "from": "pass", "to": "fail" }
  ],
  "missing": [
    { "surface": "xdr", "testId": "p28-xdr-2", "baselineStatus": "pass" }
  ],
  "added": [
    { "surface": "soroban", "testId": "p28-soroban-3", "status": "pass" }
  ]
}
```

| Field | Meaning |
|---|---|
| `diffVersion` | Integer, currently `1`. |
| `verdict` | One of `no-change`, `improved`, `coverage-lost`, `regression`. |
| `transitions` | Tests present in both reports with a different status. Sorted by `(surface, testId)` — surfaces alphabetically (`rpc`, `soroban`, `xdr`). |
| `missing` | Tests present in the baseline only (coverage lost), with the status they had there. |
| `added` | Tests present in the current report only. Omitted entirely when empty. |

Serialization is deterministic: the same pair of reports always
produces byte-identical JSON, so the output can be committed, diffed, or
hash-checked in CI.

## Example

```bash
stellar-canary check --json > current.json        # on the PR branch
git stash && stellar-canary check --json > baseline.json && git stash pop

stellar-canary diff current.json baseline.json
echo $?   # 0 clean/improved, 2 regression, 1 error
```
