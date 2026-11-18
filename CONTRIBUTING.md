# Contributing

Issues and pull requests are welcome. This is a small personal collection of tools, so for anything
bigger than a bug fix, please open an issue first to check it fits.

## Setup

```sh
git clone https://github.com/scottlandon/development-tools.git
cd development-tools
cargo test --workspace
cargo run --release -p cc-usage -- --demo      # try it without any real data
```

`rust-toolchain.toml` makes rustup install stable with rustfmt and clippy. The code builds on the
Rust version in `rust-version` (Cargo.toml); CI checks that on every push.

## Before opening a PR

```sh
cargo fmt --all
cargo clippy --workspace --all-targets      # keep it warning-free
cargo test --workspace
cargo deny check                            # optional locally; CI runs it
```

CI runs the same on macOS and Linux. Windows isn't supported yet.

## Tests

- **Unit tests** sit next to the code. `src/ui/tests.rs` renders the whole UI into an in-memory terminal at many sizes.
- **Golden frames** (`tests/golden/*.txt`) are text snapshots of whole screens. After an intended UI change, review
  the diff and regenerate: `UPDATE_GOLDEN=1 cargo test -p cc-usage golden`. Views that print local clock times are
  left out so the files don't depend on the timezone.
- **End-to-end tests** (`tests/cli.rs`) run the real binary on synthetic transcripts in `tests/fixtures/` and check
  exact totals, error messages, directory discovery and that nothing on disk changes. One test drives the TUI
  through a pseudo-terminal (macOS only).
- **Policy tests** (`tests/policy.rs`) guard the read-only promise; see below.
- **Coverage:** `cargo install cargo-llvm-cov`, then `cargo llvm-cov --workspace --summary-only`. Line coverage is
  about 99%; most of the rest is `main.rs`'s terminal setup.
- **Mutation testing** shows whether the tests would notice a wrong answer, not just whether they run the code:
  `cargo install cargo-mutants`, then e.g. `cargo mutants -p cc-usage -f '**/cc-usage/src/scan.rs'`.
  Over the logic modules (scanner, data, aggregation, app state, pricing, report, formatting) the suite catches
  over 93% of the viable mutants; the survivors we reviewed are mostly equivalent changes (for example `>` vs `>=` on a tie).

## Conventions

- Match the surrounding code: naming, comment density, idiom. `rustfmt.toml` sets the line width.
- Add tests next to the code. UI changes are covered by rendering into an in-memory terminal at many
  sizes (`tools/cc-usage/src/ui/tests.rs`).
- Keep it light. Prefer deleting code to adding it, and think hard before adding a dependency.
- New tool: `cargo new tools/<name>`, inherit the shared package fields with `edition.workspace = true`
  and friends, add a README and a row in the root README.

## The read-only rule

`cc-usage` only reads transcripts. It writes no files, opens no sockets, runs no programs, and keeps only
token counts, model names, timestamps and project folder names. `tools/cc-usage/tests/policy.rs` enforces this
on the production source and the dependency list, and `deny.toml` bans network crates. If a change needs to
break one of these, the README, `SECURITY.md` and the test have to change in the same PR, on purpose.
Filesystem access belongs in `scan.rs` and nowhere else.

## Privacy

Never put real transcripts, project names, or usage figures in issues, PRs, tests, or screenshots.
`cc-usage --demo` runs on synthetic data; the screenshots in `tools/cc-usage/docs/` are captures of it.

## Updating prices

`cc-usage` ships a built-in price table (`tools/cc-usage/src/pricing.rs`). When Anthropic changes
pricing, update the table against <https://platform.claude.com/docs/en/about-claude/pricing> and bump
`PRICES_VERIFIED` in the same commit. Check cache-read rates (not every model is 0.1× input) and any
long-prompt tiers.

A weekly workflow (`price-check.yml`) opens an issue when `PRICES_VERIFIED` is more than 60 days old.

## Releasing (maintainer)

Bump the crate version, merge, then push a tag named `<tool>-v<version>`, for example
`cc-usage-v0.2.0`. The Release workflow builds macOS and Linux binaries and attaches them, with
checksums and a signed build-provenance attestation, to a GitHub release.
