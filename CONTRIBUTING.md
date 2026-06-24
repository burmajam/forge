# Contributing to Forge

Thanks for your interest in improving Forge. This document explains how to contribute and the one
legal step that is required before your code can be merged.

## Contributor License Agreement (CLA)

Forge is [dual-licensed](LICENSING.md): AGPL-3.0 for everyone, and a commercial license for those
who need to build closed-source products on top of it. For that model to work, the project
maintainer must hold the rights to relicense the entire codebase.

**Before your first contribution can be merged, you must sign the [Contributor License
Agreement](CLA.md).** Signing is automated: when you open your first pull request, a bot
([CLA Assistant](https://github.com/cla-assistant/cla-assistant)) will comment with a link. You
sign once; it applies to all your future contributions.

The CLA grants the maintainer a broad license to your contribution (including the right to
distribute it under both the AGPL and the commercial license). You retain copyright to your work.

Contributions are made **voluntarily and without any expectation of compensation**. The CLA does
not entitle you to payment.

## How to contribute

1. **Open an issue first** for anything non-trivial, so we can agree on the approach before you
   invest time. Issues are tracked at [github.com/kadabra-ai/forge/issues](https://github.com/kadabra-ai/forge/issues).
2. **Fork and branch** — never work on `main`. Use a descriptive branch name.
3. **Make your change**, keeping it to one logical change per pull request.
4. **Run the checks** before pushing:
   ```bash
   cargo build
   cargo test
   cargo clippy --all-targets -- -D warnings
   cargo fmt --check
   ```
5. **Open a pull request** against `main` and sign the CLA when prompted.

## Code standards

- Fix every warning from every tool — clippy, the compiler, tests. Clean output is the baseline.
- Match the style of the surrounding code.
- Test behavior, not implementation. Cover edges and error paths, not just the happy path.
- Write self-documenting code; delete commented-out code.

## Commits

- Imperative mood, ≤72-character subject line.
- One logical change per commit.
- Never commit secrets, keys, or credentials.

## Questions

Open an issue, or for licensing questions email
[licensing@kadabra.rs](mailto:licensing@kadabra.rs).
