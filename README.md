# Sessio

`Sessio` translates interactive session history between Codex, Claude Code, and Droid CLI.

The default workflow is direct native-to-native conversion by session id:

```bash
sessio --from claude --to codex <SESSION_ID>
sessio --from codex --to claude <SESSION_ID>
sessio --from droid --to claude <SESSION_ID>
sessio --from claude --to droid <SESSION_ID>
```

By default, `Sessio`:

- resolves the source session id from the local Codex, Claude, or Droid store
- creates a fresh target session id automatically
- writes the translated session into the target tool's storage
- immediately opens the translated session in the target agent

If you only want the translation and do not want to start the target agent yet:

```bash
sessio --from claude --to codex <SESSION_ID> --no-open
sessio --from codex --to claude <SESSION_ID> --no-open
sessio --from droid --to codex <SESSION_ID> --no-open
```

## Install

Install directly from GitHub:

```bash
cargo install --git https://github.com/KilimcininKorOglu/sessio.git
```

Or from a local checkout:

```bash
cargo install --path .
```

After the crate is published on crates.io, the standard Cargo install command will be:

```bash
cargo install sessio
```

For development:

```bash
git clone https://github.com/KilimcininKorOglu/sessio.git
cd sessio
make build
```

The development build target writes the CLI binary to `bin/sessio`.

## Quick Start

Convert a Claude session into Codex and open the translated Codex session immediately:

```bash
sessio --from claude --to codex <CLAUDE_SESSION_ID>
```

Convert a Codex session into Claude and open the translated Claude session immediately:

```bash
sessio --from codex --to claude <CODEX_SESSION_ID>
```

Convert a Droid session into Claude and open the translated Claude session immediately:

```bash
sessio --from droid --to claude <DROID_SESSION_ID>
```

Convert a Claude or Codex session into Droid and open it immediately:

```bash
sessio --from claude --to droid <CLAUDE_SESSION_ID>
sessio --from codex --to droid <CODEX_SESSION_ID>
```

If you want the translated session to be written somewhere else first, override the target root explicitly:

```bash
sessio --from claude --to codex <SESSION_ID> --output ./tmp/codex-home
sessio --from codex --to claude <SESSION_ID> --output ./tmp/claude-home
sessio --from claude --to droid <SESSION_ID> --output ./tmp/factory-home
```

When opening after translation, `Sessio` launches the target CLI with the translated session id. For custom output roots, it sets `CODEX_HOME` for Codex, `CLAUDE_CONFIG_DIR` plus `CLAUDE_HOME` for Claude, and `FACTORY_HOME` plus `DROID_HOME` for Droid.

For Codex custom output roots, `Sessio` also links the installed `auth.json` into the target home when needed so the launched Codex process can authenticate immediately.

## Bulk Conversion

Bulk conversion is intentionally dry-run first. A bulk command discovers native sessions from the selected source store, converts them into a temporary target home, validates the generated files, and leaves the real target store untouched unless `--apply` is passed:

```bash
sessio bulk --from claude --to droid --dry-run
sessio bulk --from droid --to claude --dry-run
sessio bulk --from codex --to droid --dry-run
```

After the dry run succeeds, write to an explicit target home:

```bash
sessio bulk --from droid --to claude --apply --output ./tmp/claude-home
sessio bulk --from claude --to droid --apply --output ./tmp/factory-home
```

To write into the configured target store, run:

```bash
sessio bulk --from codex --to droid --apply
```

Bulk conversion supports Codex, Claude, and Droid as native source and target formats. The source and target must be different native formats. Portable IR is intentionally not a bulk source or target. Bulk apply refuses standalone `.jsonl` outputs and stops before writing if target session files already exist. Droid settings sidecars are treated as target conflicts; existing Codex `session_index.jsonl` and Claude `history.jsonl` files are allowed because those files are append-only indexes. Bulk conversion never opens target agents automatically.

## Session Lookup

For Codex, Claude, and Droid inputs, `Sessio` accepts either:

- a native session id
- a direct session file path

By default it searches:

- Codex: `SESSIO_CODEX_HOME`, then legacy `TRANSESSION_CODEX_HOME`, then `CODEX_HOME`, then `~/.codex`
- Claude: `SESSIO_CLAUDE_HOME`, then legacy `TRANSESSION_CLAUDE_HOME`, then `CLAUDE_CONFIG_DIR`, then `CLAUDE_HOME`, then `~/.claude`
- Droid: `SESSIO_DROID_HOME`, then legacy `TRANSESSION_DROID_HOME`, then `DROID_HOME`, then `FACTORY_HOME`, then `~/.factory`

That means you can usually use the same id you would pass to `codex resume`, `claude -r`, or `droid -r`.

## What Gets Preserved

`Sessio` preserves the main conversation state needed for practical handoff:

- user and assistant messages
- reasoning summaries
- tool calls and tool results
- timestamps
- working directory and branch hints
- lightweight platform metadata needed for native session discovery

## Caveats

`Sessio` intentionally focuses on the durable conversation logs and lightweight resume metadata. It does not recreate every platform-specific side channel.

The current test suite covers the main happy paths, but real-world session logs are messy and platform behavior keeps evolving. You should expect some edge cases and translation failures to surface over time, and the converter will likely need further iteration as those cases are discovered.

Known omissions:

- opaque reasoning payloads and token-accounting side data
- Codex SQLite state and shell snapshot sidecars
- Claude subagent trees and tool-result sidecar directories
- Droid runtime caches outside the session JSONL and settings sidecar
- platform-specific runtime caches outside the main session log

## Development

Local development uses the Makefile:

```bash
make build
make run ARGS="--from claude --to droid <SESSION_ID> --no-open"
cargo run -- bulk --from claude --to droid --dry-run
make fmt
make clippy
make test
make check
```

Useful targets:

- `make build` builds the release binary and writes it to `bin/sessio`
- `make run ARGS="..."` builds and runs `bin/sessio`
- `make test-one TEST=<test_name>` runs one integration test from `tests/roundtrip.rs`
- `make publish-dry-run` runs `cargo publish --dry-run --locked`
- `make clean` removes Cargo build output and `bin/`

The underlying local checks are:

```bash
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

Pre-commit hooks are configured in `.pre-commit-config.yaml`.

To enable them locally:

```bash
pipx install pre-commit
pre-commit install
```

The configured hooks run:

- `cargo fmt --all --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test`

GitHub Actions workflows are included:

- `.github/workflows/ci.yml` for formatting, linting, and tests
- `.github/workflows/publish.yml` for dry-run or real crates.io publishing

## Publishing

The repository is prepared for `cargo install sessio` once the crate is published.

What you need to do before the real publish:

1. Create a crates.io API token with publish permission.
2. Add that token to the GitHub repository secrets as `CARGO_REGISTRY_TOKEN`.
3. Make sure the version in `Cargo.toml` is the version you want to release.
4. Push the release commit to `master`.

How to run the publish workflow:

- For a dry run in GitHub Actions: open the `publish` workflow and run `workflow_dispatch` with `dry_run=true`.
- For a real publish in GitHub Actions: run `workflow_dispatch` with `dry_run=false`, or push a tag like `v0.1.2`.

The publish workflow will:

- verify formatting
- run clippy with `-D warnings`
- run tests
- verify that a pushed `vX.Y.Z` tag matches the `Cargo.toml` version
- run `cargo publish --locked`

The crate name `Sessio` appeared available during the latest local check, and `cargo publish --dry-run` succeeded locally. You should still treat name availability as time-sensitive until the first real publish completes.

## Advanced Usage

There is also a portable intermediate representation for debugging and advanced workflows, but it is intentionally not the main interface.

Advanced commands remain available:

```bash
sessio bulk --from claude --to droid --dry-run
sessio bulk --from droid --to claude --apply --output ./tmp/claude-home
sessio inspect <SESSION_ID> --from claude
sessio inspect <SESSION_ID> --from droid
sessio import <SESSION_ID> ./session.json --from codex
sessio import <SESSION_ID> ./session.json --from droid
sessio export ./session.json ./out/codex-home --to codex --new-session-id
sessio export ./session.json ./out/factory-home --to droid --new-session-id
sessio convert <SESSION_ID> ./out/claude-home --from droid --to claude --new-session-id
```

## AI Disclaimer

This project was built with Codex. The code and documentation were generated and refined collaboratively with AI assistance, then validated locally with tests and CLI smoke checks.
