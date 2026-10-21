# `lock`

Pins the project's test dependencies for reproducible execution: writes
a `.stellar-canary.lock` at the project root capturing the active
fixture pack, the target protocol, and the tool version (CF-03).

```text
Pin the current fixture pack, protocol, and tool version in a `.stellar-canary.lock` (CF-03)

Usage: stellar-canary lock [OPTIONS]

Options:
      --protocol <PROTOCOL>        Target protocol version (overrides configuration)
      --fixtures-dir <DIR>         Directory containing fixture files [default: fixtures]
      --config <PATH>              Path to a configuration file (default: .stellar-canary.toml in the project root)
  -h, --help                       Print help
```

## The lockfile

```toml
lockVersion = 1

[tool]
version = "0.2.0"

[fixtures]
protocol = 28
packDigest = "sha256:…"
registryVersion = 1
source = "directory"
```

| Field | Meaning |
|---|---|
| `lockVersion` | Integer, currently `1`. A future format change bumps this. |
| `tool.version` | The Canary CLI version that produced the lock (`MAJOR.MINOR.PATCH`). |
| `fixtures.protocol` | The pinned target protocol (from `--protocol`, else the configuration file, else the default). |
| `fixtures.packDigest` | The pack's canonical content digest (`canary-pack-v1`, CF-02): the ASCII header `canary-pack-v1`, then one `<sha256> <size> <relative path>` line per pack file, sorted by pack-relative path. Independent of where the pack is checked out — identical layout and content produce the same digest on any machine. |
| `fixtures.registryVersion` | The pinned pack's registry format version (required; currently `1`). |
| `fixtures.source` | Where the pack came from (`directory` here). Informational only. |

Serialization is deterministic (CF-03 section 3): keys in the contract's
fixed order, `\n` line endings, a trailing newline and no comments, so
the same pack always produces a byte-identical lockfile and
regenerating it in CI and diffing is a reliable drift check.

## Safety

The entire pack is resolved and validated **before** the lockfile is
touched. If the fixtures directory does not exist, or the pack is
invalid (duplicate ids, dangling file references, unparseable TOML),
`lock` reports a clear error using the standard
[exit-code contract](../exit-codes.md) and leaves **no partial
lockfile** behind.

## Example

```bash
stellar-canary lock --fixtures-dir ProtocolCanary-Fixtures/protocol-28
# Wrote /path/to/project/.stellar-canary.lock (lockVersion 1, protocol 28, packDigest sha256:…)
```
