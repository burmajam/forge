# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**forge** is a Cargo-style project management CLI for KerML/SysML v2 systems modeling projects. The core is **kermlc**,
a KerML compiler written in Rust targeting the KerML 1.0 Beta 2 specification. The compiler parses KerML textual 
notation, resolves names, type-checks, validates, and serializes to JSON-LD (with XMI planned).

## Build & Test Commands

```bash
cargo build                               # Build all crates
cargo test                                # Run all tests
cargo test -p kermlc_parser               # Test a single crate
cargo test -p kermlc -- integration       # Run integration tests only
cargo test -p kermlc_parser -- test_name  # Run a single test by name
cargo clippy --all-targets -- -D warnings # Lint
cargo fmt --check                         # Format check
```

## Workspace Architecture

13-crate Cargo workspace under `crates/`. Two thin driver binaries sit over one shared,
language-agnostic engine (`harpoon`); the front-end is language-specific. See ADR-0001 / `CONTEXT.md`.
The compiler pipeline flows linearly with a fixpoint loop inside `harpoon::compile()`:

```
Source (.kerml)
  -> kermlc_lexer        (tokenization)                          [KerML front-end]
  -> kermlc_parser       (recursive descent -> AST)              [KerML front-end]
  -> kermlc_lower        (AST lowering -> SemanticModel)         [KerML front-end]
  -> load_stdlib + add_implicit_specializations                  (driver prelude)
  -> harpoon::compile()  (engine entry point):
       harpoon_resolve   (name resolution, 5-strategy)
         <-> harpoon_typeck (type checking, interleaved fixpoint, max 100 iterations)
       emit_unresolved_errors + detect_specialization_cycles
       harpoon_validate  (semantic validation)
  -> harpoon_serial_json (JSON-LD serialization)                 (driver, downstream of model)
```

The driver wrapper is `kermlc::compile_source()` (parse → lower → prelude → `harpoon::compile`).

### Crate Dependency Order (bottom-up)

| Layer        | Crate                 | Key Exports                                                                       |
|--------------|-----------------------|----------------------------------------------------------------------------------|
| Foundation   | `harpoon_intern`      | `StringInterner`, `SymbolId`, `Idx<T>`, `Arena<T>`                                |
| Foundation   | `harpoon_diagnostics` | `DiagnosticSink`, `SourceMap`, `Span`, `FileId`                                   |
| Frontend     | `kermlc_ast`          | AST node types, `Idx<T>` aliases (`PackageId`, `TypeDeclId`, `FeatureDeclId`)     |
| Frontend     | `kermlc_lexer`        | `Token`, `TokenKind`, `Lexer`                                                     |
| Frontend     | `kermlc_parser`       | `Parser::parse()` -> `ParseResult`                                               |
| Frontend     | `kermlc_lower`        | `lower_ast()` (KerML AST -> `SemanticModel`)                                      |
| Engine       | `harpoon_hir`         | `SemanticModel`, `Def`, `DefId`, `MembershipId`, `load_stdlib()`, `FeatureDirection`, `Visibility` |
| Engine       | `harpoon_resolve`     | `resolve_pass()`, `detect_specialization_cycles()`, `emit_unresolved_errors()`   |
| Engine       | `harpoon_typeck`      | `typecheck_pass()`                                                                |
| Engine       | `harpoon_validate`    | `validate()`                                                                      |
| Engine       | `harpoon_serial_json` | `serialize_to_json()`                                                             |
| Engine facade| `harpoon`             | `compile()` (fixpoint + diagnostics + validate); re-exports engine types         |
| Binary       | `kermlc`              | CLI entry point, `compile_source()` (lib.rs) + thin `main.rs`                     |

Engine crates (`harpoon_*`) never depend on a front-end crate; both front-ends target `SemanticModel`.

### Core Data Model

- **Index-based arenas** (`Arena<T>` returning `Idx<T>` handles; `DefId = Idx<Def>`, `MembershipId = Idx<Membership>`) plus `u32` newtypes (`SymbolId`, `FileId`) — no lifetime parameters on data structures
- **`SemanticModel`** is the central mutable structure passed through resolve/typeck/validate
- **Fixpoint loop**: `resolve_pass()` and `typecheck_pass()` alternate until neither makes progress (returns `bool` for "changed")
- **Stdlib**: 6 hardcoded types (Anything, Object, DataValue, Occurrence, Performance, Link) injected via `load_stdlib()` — no file I/O yet

### Compilation Pipeline (integration test pattern)

The whole pipeline is one call. Tests and the binary cross the same seam:

```rust
let compiled = kermlc::compile_source(source, "test.kerml");
assert!(!compiled.sink.has_errors());
// compiled.model / compiled.interner / compiled.source_map are available for inspection.
```

`compile_source` (in `crates/kermlc/src/lib.rs`) runs: parse -> `lower_ast` -> `load_stdlib` ->
`add_implicit_specializations` -> `harpoon::compile`. `harpoon::compile(&mut model, &interner,
&mut sink)` owns the resolve/typeck fixpoint, then `emit_unresolved_errors`,
`detect_specialization_cycles`, and `validate` — callers never choreograph those in sequence.
Serialization (`serialize_to_json`) is a downstream step the driver invokes after the model is built.

## Test Fixtures

Integration test fixtures live in `crates/kermlc/tests/fixtures/`:
- `valid/` — `.kerml` files that should compile without errors
- `invalid/` — `.kerml` files that should produce specific diagnostics

## Vendor Resources (OMG Specifications)

The `vendor/SysML-v2-Release/` folder contains the official OMG specification materials:

| Path                                                       | Contents                                                                               |
|------------------------------------------------------------|----------------------------------------------------------------------------------------|
| `doc/`                                                     | KerML and SysML v2 specification PDFs, intro guides                                    |
| `bnf/`                                                     | Official KerML and SysML textual/graphical BNF grammars (`.kebnf`, `.html`)            |
| `kerml/src/examples/`                                      | KerML example models (Vehicle, Associations, Behaviors, Spec Annex A, etc.)            |
| `sysml.library/Kernel Libraries/Kernel Semantic Library/`  | Official `.kerml` stdlib files (Base, Objects, Occurrences, Performances, Links, etc.) |
| `sysml.library/Kernel Libraries/Kernel Data Type Library/` | Standard data type definitions                                                         |
| `sysml.library/Kernel Libraries/Kernel Function Library/`  | Standard function definitions                                                          |
| `sysml.library/Domain Libraries/`                          | Domain-specific libraries                                                              |
| `sysml.library/Systems Library/`                           | Systems-level libraries                                                                |

Use `bnf/KerML-textual-bnf.kebnf` as the authoritative grammar reference. Use `kerml/src/examples/` for integration test sources. Use `sysml.library/Kernel Libraries/Kernel Semantic Library/` for B2 (stdlib-from-files) implementation.

IMPORTANT: 
- The big spec PDFs live in `vendor/SysML-v2-Release/doc/`. High-fidelity Markdown conversions (Marker, code blocks fenced + figures extracted) are in `docs/spec/<name>/<name>.md` — use these for quick search/reference. Each folder also holds the linked `_page_*.jpeg` figures and a `_meta.json` (page stats + TOC). See `docs/spec/README.md` for provenance and how to regenerate. For ambiguous figures, fall back to the source PDF via the Read tool's `pages` parameter.
- `vendor/SysML-v2-Release/bnf/KerML-textual-bnf.kebnf` contains grammar rules.
- `vendor/KerML-v1/` holds the KerML 1.0 kernel Data Type + Function libraries (`.kerml` + `.kpar` archives) — source material for B2 (stdlib-from-files) and D3 (KPAR).

## Progress Tracking

Current progress is tracked in `docs/plans/progress.md`. Check this file at the start of each session when continuing work. The design spec lives at `docs/plans/2026-02-15-kermlc-compiler-design.md`.

## KerML Grammar Subset (Currently Implemented)

```
Package        = 'package' QualifiedName '{' PackageBody '}'
TypeDecl       = 'type' Name Specialization? Conjugation? '{' TypeBody '}'
FeatureDecl    = 'feature' Name ':' TypeRef FeatureChain? Multiplicity? ';'
Import         = 'import' QualifiedName ('::*')? ';'
Specialization = ('specializes' | ':>') QualifiedName (',' QualifiedName)*
Conjugation    = 'conjugates' | '~' QualifiedName
Multiplicity   = '[' Expr '..' Expr ']' | '[' Expr ']'
```

Feature directions (`in`, `out`, `inout`) are parsed and stored. Conjugation flips `in` <-> `out` at type level: the target is stored in `Def.conjugation`, and directions are flipped when resolving a feature's direction (`conjugate_dir`) over the type's `inherited_memberships`.

## Agent skills

### Issue tracker

Issues and PRDs are tracked as GitHub issues in `mjaric/forge` via the `gh` CLI. External PRs are not a triage surface. See `docs/agents/issue-tracker.md`.

### Triage labels

Canonical triage vocabulary (`needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`); `wontfix` already exists, the rest are created on first use. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: one `CONTEXT.md` + `docs/adr/` at the repo root. See `docs/agents/domain.md`.
