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

11-crate Cargo workspace under `crates/`. The compiler pipeline flows linearly with a fixpoint loop in the middle:

```
Source (.kerml)
  -> kermlc_lexer       (tokenization)
  -> kermlc_parser      (recursive descent -> AST)
  -> kermlc_hir         (AST lowering + stdlib loading -> SemanticModel)
  -> kermlc_resolve     (name resolution, 5-strategy)
     <-> kermlc_typeck  (type checking, interleaved fixpoint loop, max 100 iterations)
  -> kermlc_validate    (semantic validation)
  -> kermlc_serial_json (JSON-LD serialization)
```

### Crate Dependency Order (bottom-up)

| Layer      | Crate                | Key Exports                                                                                 |
|------------|----------------------|---------------------------------------------------------------------------------------------|
| Foundation | `kermlc_intern`      | `StringInterner`, `SymbolId`                                                                |
| Foundation | `kermlc_diagnostics` | `DiagnosticSink`, `SourceMap`, `Span`, `FileId`                                             |
| Frontend   | `kermlc_ast`         | AST node types, `AstArena`, `AstId<T>`                                                      |
| Frontend   | `kermlc_lexer`       | `Token`, `TokenKind`, `Lexer`                                                               |
| Frontend   | `kermlc_parser`      | `Parser::parse()` -> `ParseResult`                                                          |
| Semantic   | `kermlc_hir`         | `SemanticModel`, `DefArena`, `DefId`, `TypeArena`, `TypeId`, `lower_ast()`, `load_stdlib()` |
| Semantic   | `kermlc_resolve`     | `resolve_pass()`, `detect_specialization_cycles()`, `emit_unresolved_errors()`              |
| Semantic   | `kermlc_typeck`      | `typecheck_pass()`                                                                          |
| Semantic   | `kermlc_validate`    | `validate()`                                                                                |
| Backend    | `kermlc_serial_json` | `serialize_to_json()`                                                                       |
| Binary     | `kermlc`             | CLI entry point, `pipeline::resolve_and_typecheck()`                                        |

### Core Data Model

- **Index-based arenas** with `u32` newtypes (`DefId`, `TypeId`, `AstId<T>`, `SymbolId`, `FileId`) — no lifetime parameters on data structures
- **`SemanticModel`** is the central mutable structure passed through resolve/typeck/validate
- **Fixpoint loop**: `resolve_pass()` and `typecheck_pass()` alternate until neither makes progress (returns `bool` for "changed")
- **Stdlib**: 6 hardcoded types (Anything, Object, DataValue, Occurrence, Performance, Link) injected via `load_stdlib()` — no file I/O yet

### Compilation Pipeline (integration test pattern)

```rust
let mut interner = StringInterner::new();
let mut source_map = SourceMap::new();
let mut sink = DiagnosticSink::new();
let file_id = source_map.add_file("test.kerml".into(), source.into());
let parse = Parser::parse(source, file_id, &mut interner, &mut sink);
let mut model = lower_ast(&parse, &interner, &mut sink);
let stdlib = load_stdlib(&mut model, &mut interner);
add_implicit_specializations(&mut model, &stdlib);
// fixpoint loop
for _ in 0..100 {
    let r = resolve_pass(&mut model, &interner, &mut sink);
    let t = typecheck_pass(&mut model, &interner, &mut sink);
    if !r && !t { break; }
}
emit_unresolved_errors(&mut model, &interner, &mut sink);
detect_specialization_cycles(&mut model, &interner, &mut sink);
validate(&model, &interner, &mut sink);
```

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

IMOPRTANT: `vendor/SysML-v2-Release/doc` folder pdf files are big, so there are markdown versions without images. Use the markdown versions for quick reference, quick search, etc.

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

Feature directions (`in`, `out`, `inout`) are parsed and stored. Conjugation flips `in` <-> `out` at type level via `InheritedFeature` with `direction_override`.
