# kermlc — KerML Compiler Design

## Goal

Build a full KerML compiler in Rust targeting the KerML 1.0 Beta 2 specification (SysML v2 Release, 2024). The compiler parses KerML textual notation, resolves names, type-checks, validates, and serializes to pluggable backends (JSON-LD, XMI).

## Key Decisions

- **Language:** Rust
- **Parser:** Hand-written recursive descent
- **Arenas:** Index-based (typed newtypes around `u32`, no lifetime parameters on data structures)
- **Pipeline:** Iterative fixpoint for interleaved name resolution and type checking
- **Milestone strategy:** Full pipeline with minimal grammar first, then grow grammar. The minimal grammar subset exercises all pipeline-breaking KerML features.
- **SysML readiness:** KerML crates are libraries. SysML v2 (and later v1 import) will layer on top by lowering to KerML HIR.
- **Project tooling:** `forge` — a Cargo-style CLI for systems modeling projects (KerML, SysML, OCL). Handles project manifests, dependency resolution, KPAR archive compliance, and repository integration (OMG Systems Modeling API and Services). The compiler (`kermlc`) is a library; `forge` handles project management.
- **Package format:** KPAR (KerML Project Archive) — OMG normative ZIP format with `.project.json` and `.meta.json`. Used for publishing/sharing. Local development uses a `KerML.toml` manifest.

## Pipeline-Breaking KerML Features

These features have deep pipeline implications and must be accommodated from day one, even if not fully implemented in the first milestone:

1. **Type-directed name resolution** — Feature chaining (`a.b.c`) requires type info to resolve names. Name resolution and type checking are interleaved, not sequential.
2. **Implicit specialization** — Every type implicitly specializes a Kernel Semantic Library type. The compiler synthesizes relationships not in source text.
3. **Kernel Semantic Library loading** — Base types (`Anything`, `Object`, `Performance`, `DataValue`, etc.) must be pre-loaded.
4. **Diamond inheritance** — Multiple specialization creates diamond patterns. Membership resolution must be ordering-independent.
5. **Conjugation** — Type transformation flipping input/output directionality. First-class type system operation.
6. **Feature redefinition chains** — Features are redefined in subtypes; behavior parameters implicitly redefine matching parameters.
7. **Visibility + feature chains interaction** — `protected` visibility interacts with feature chaining in non-trivial ways.
8. **Multi-file / package compilation** — KerML models span files with cross-file references.

## High-Level Architecture

```
Source Files (.kerml)
       │
       ▼
   ┌─────────┐
   │  Lexer   │  → Token stream
   └────┬─────┘
        │
   ┌────▼─────┐
   │  Parser   │  → Untyped AST (in AstArena)
   └────┬─────┘
        │
   ┌────▼──────────────┐
   │  AST Lowering      │  → Semantic model skeleton (in DefArena + TypeArena)
   │  + Library Loading  │     Loads Kernel Semantic Library
   └────┬──────────────┘
        │
   ┌────▼──────────────────────┐
   │  Resolution Loop (fixpoint)│
   │  ┌──────────┐ ┌─────────┐ │
   │  │ Name Res │↔│ Type Ck ││  → Resolved + typed semantic model
   │  └──────────┘ └─────────┘ │
   └────┬──────────────────────┘
        │
   ┌────▼─────────┐
   │  Validation   │  → Diagnostics (errors, warnings)
   └────┬─────────┘
        │
   ┌────▼─────────────┐
   │  Serialization    │  → JSON-LD / XMI / API
   │  (pluggable)      │
   └──────────────────┘
```

## Crate Structure (Cargo Workspace)

| Crate | Purpose |
|-------|---------|
| `kermlc` | Compiler binary (like rustc) |
| `kermlc_lexer` | Tokenization |
| `kermlc_parser` | Recursive descent parser → AST |
| `kermlc_ast` | AST node definitions + AstArena |
| `kermlc_hir` | High-level IR / semantic model + DefArena, TypeArena |
| `kermlc_resolve` | Name resolution |
| `kermlc_typeck` | Type checking + conjugation |
| `kermlc_validate` | Semantic validation |
| `kermlc_diagnostics` | Error/warning types, rendering, source spans |
| `kermlc_serial_json` | JSON-LD serialization backend |
| `kermlc_serial_xmi` | XMI serialization backend |
| `kermlc_intern` | String interner + index-based arena infrastructure |
| `kermlc_project` | Manifest parsing, dependency resolution, package fetching |

**Separate binary (different repo/workspace later):**

| Crate | Purpose |
|-------|---------|
| `forge` | Project management CLI (like cargo). Invokes `kermlc` for compilation. |
| `forge_registry` | Systems Modeling API client, KPAR archive read/write |

All `kermlc_*` crates except `kermlc` (the binary) are libraries, enabling SysML v2/v1 and `forge` to build on top.

## SysML Layering

SysML v2 compiles down to KerML HIR. The SysML compiler adds its own parser and lowering phase, then reuses kermlc's resolution, type checking, validation, and serialization:

```
SysML v2 source (.sysml)          KerML source (.kerml)
        │                                  │
   sysmlc_parser                     kermlc_parser
        │                                  │
   SysML AST                          KerML AST
        │                                  │
   sysmlc_lower ──────────────┐            │
                              ▼            ▼
                    ┌─── KerML HIR (shared) ───┐
                    │  resolve ↔ typeck loop   │
                    │  validation               │
                    │  serialization             │
                    └──────────────────────────┘
```

SysML v1 import: separate crate reads XMI, produces KerML HIR.

## Data Model & Arenas

### String Interning
- `StringInterner` deduplicates all identifiers and string literals
- Returns `SymbolId` (newtype `u32`) — cheap integer equality comparison
- Single interner shared across compilation

### Source Spans
- `Span { file: FileId, start: u32, end: u32 }` — byte offsets
- `FileId` indexes into a file table in the `SourceMap`
- `SourceMap` retains original source text per file for diagnostic rendering

### AST Arena
- `AstArena` stores all AST nodes, immutable after parsing
- `AstId<T>` — typed index (e.g., `AstId<TypeDecl>`, `AstId<FeatureDecl>`)
- Each node carries a `Span`

### HIR / Semantic Model
- `DefArena` — definitions (packages, types, features, relationships)
- `DefId` — index into `DefArena`
- `TypeArena` — resolved types
- `TypeId` — index into `TypeArena`
- Nodes start partially resolved, filled in across fixpoint iterations
- Each `DefId` links back to its `AstId` for diagnostics

### Resolution State

```rust
enum ResolutionState {
    Unresolved,
    InProgress,   // cycle detection
    Resolved,
    Error,
}
```

Each definition carries its resolution state. The fixpoint loop iterates until all nodes are `Resolved` or `Error`.

## Lexer & Parser

### Lexer
- Hand-written, produces `Token { kind: TokenKind, span: Span }`
- `TokenKind` covers KerML keywords, operators (`:>`, `~`, `::`, `.`), literals, identifiers
- Lazy iteration (one token at a time)
- Comments preserved in token stream for source map fidelity

### Parser
- Hand-written recursive descent
- Pratt parsing for expressions (operator precedence)
- **Error recovery:** on unexpected token, record diagnostic, skip to synchronization point (`;`, `}`, keyword), continue parsing
- **Missing token recovery:** insert synthetic token, record diagnostic, continue
- Always produces an AST (with error nodes if needed) — downstream phases check for errors

### Initial Grammar Subset (Milestone 1)

```
Package        = 'package' QualifiedName '{' PackageBody '}'
PackageBody    = (Import | TypeDecl | FeatureDecl | Package)*
Import         = 'import' QualifiedName ('::*')? ';'
TypeDecl       = 'type' Name Specialization? Conjugation? '{' TypeBody '}'
Specialization = ('specializes' | ':>') QualifiedName (',' QualifiedName)*
Conjugation    = 'conjugates' | '~' QualifiedName
TypeBody       = (FeatureDecl | TypeDecl)*
FeatureDecl    = 'feature' Name ':' TypeRef FeatureChain? Multiplicity? ';'
FeatureChain   = 'chains' QualifiedName ('.' QualifiedName)*
Multiplicity   = '[' Expr '..' Expr ']' | '[' Expr ']'
Expr           = literal | name | Expr op Expr | '(' Expr ')'
QualifiedName  = Name ('::' Name)*
```

This subset exercises: packages/namespaces, imports, specialization, conjugation, feature chaining, multiplicity, and basic expressions.

## Diagnostics

### Diagnostic Structure

```rust
struct Diagnostic {
    severity: Severity,           // Error, Warning, Info, Hint
    message: String,              // Primary message
    code: DiagnosticCode,         // E0001, W0012, etc.
    labels: Vec<Label>,           // Annotated source spans
    notes: Vec<String>,           // Additional context
    suggestions: Vec<Suggestion>, // "did you mean...?" fixes
}

struct Label {
    span: Span,
    message: String,
    style: LabelStyle,  // Primary (^^^), Secondary (---)
}

struct Suggestion {
    message: String,
    span: Span,
    replacement: String,
}
```

### Rendering

Rustc-style terminal output with source snippets, underline arrows, and suggestions:

```
error[E0023]: unknown type `Vehicel`
  --> model.kerml:12:24
   |
12 |   feature engine : Vehicel;
   |                    ^^^^^^^ not found in this scope
   |
   = help: did you mean `Vehicle`?
```

### Error Accumulation
- All phases append to a shared `Vec<Diagnostic>`
- After all phases, diagnostics sorted by file/line, deduplicated, rendered
- Compilation continues through errors (collecting as many as possible) but stops before serialization if any errors exist

## Resolution & Type Checking (Fixpoint Loop)

### AST Lowering (before the loop)
1. Walk the AST, create skeleton `Def` entries in `DefArena`
2. Establish parent-child ownership (package → types → features)
3. Record explicit specialization/conjugation as unresolved name references
4. Load Kernel Semantic Library into `DefArena` (pre-resolved)
5. Insert implicit specialization edges (every type → its library base type)

### Fixpoint Loop

```
loop {
    changed = false

    // Pass 1: Name Resolution
    for each unresolved name reference:
        try to resolve using:
          1. Local scope (enclosing namespace)
          2. Inherited members (via specialization)
          3. Imports
          4. Feature chaining (requires type info — may defer)
        if resolved: link to DefId, changed = true

    // Pass 2: Type Checking
    for each definition with newly resolved references:
        compute/refine its type
        check specialization validity
        check conjugation consistency
        check redefinition compatibility
        if type refined: changed = true

    if !changed: break  // fixpoint reached
}
```

### After the Loop
- Remaining `Unresolved` nodes → emit diagnostics
- `InProgress` nodes → circular reference errors
- Validated model passed downstream

### Conjugation Handling
- Type `T` conjugates type `U` → input features of `U` become output features of `T` and vice versa
- Stored as `ConjugatedType { original: TypeId, conjugate_of: DefId }` in `TypeArena`
- Type checking verifies conjugation is applied to compatible types

### Feature Chaining Resolution
- `feature cousins chains parents.siblings.children`
- Resolve `parents` → get type `T1`; look up `siblings` in `T1` → get type `T2`; look up `children` in `T2` → chain result type
- If any step unresolved, defer to next fixpoint iteration

## Validation

Runs after resolution completes:
- **Multiplicity checks**: consistent with redefinitions and specializations
- **Disjointness checks**: disjoint types don't have common specializations
- **Redefinition validity**: redefined features compatible with originals
- **Completeness checks**: required features present, no dangling references
- **Library conformance**: implicit specializations correctly inherited
- Each check produces diagnostics; collects all, doesn't stop on first error

## Serialization

Pluggable backends behind a trait:

```rust
trait SerializationBackend {
    fn serialize(&self, model: &SemanticModel, out: &mut dyn Write) -> Result<()>;
}
```

- **JSON-LD backend** (first priority): SysML v2 standard API JSON format
- **XMI backend** (later): MOF-compatible XMI for traditional tool interop

## Project Management & Package System

### Two Tools

- **`kermlc`** — the compiler. Takes source files + include paths, produces output. Like `rustc`.
- **`forge`** — the project management tool. Like `cargo`. Handles projects, dependencies, KPAR archives, registry interaction. Invokes `kermlc` for compilation.

### KPAR Compliance

Published/shared packages use the OMG normative **KPAR** (KerML Project Archive) format:
- ZIP archive containing `.project.json`, `.meta.json`, and model content
- Compliant with OMG Systems Modeling API and Services spec
- Interoperable with other SysML v2 tools in the ecosystem

### Project Manifest (`KerML.toml`)

Local development uses a TOML manifest (similar to `Cargo.toml`):

```toml
[package]
name = "my-vehicle-model"
version = "0.1.0"

[dependencies]
automotive-library = { version = "1.2", registry = "https://repo.example.com" }
common-types = { path = "../common-types" }

[build]
stdlib = "bundled"  # or a path, or "none"
```

### Forge CLI Commands

| Command | Purpose |
|---------|---------|
| `forge new <name>` | Create a new project |
| `forge check` | Parse + resolve + validate (invokes kermlc) |
| `forge build` | Full compilation to output format (invokes kermlc) |
| `forge add <dep>` | Add a dependency |
| `forge publish` | Package as KPAR and publish to a registry |
| `forge fetch` | Download dependencies |

### Dependency Sources

- **Local path** — `{ path = "..." }`
- **Registry** — Systems Modeling API and Services compatible (REST API)
- **KPAR archive** — direct `.kpar` file reference
- **Bundled stdlib** — shipped with kermlc

### Compiler Interface

`kermlc` receives pre-resolved paths from `forge` (or can be used standalone):

```rust
fn compile(config: CompileConfig) -> CompileResult;

struct CompileConfig {
    source_files: Vec<PathBuf>,
    stdlib_path: PathBuf,
    dependency_paths: Vec<(String, PathBuf)>,
    output: OutputConfig,
}
```

The compiler never fetches anything — `forge` / `kermlc_project` resolves all dependencies and passes paths to the compiler.

### kermlc Standalone Usage

```
kermlc check model.kerml --stdlib /path/to/stdlib
kermlc compile model.kerml -o out.json --stdlib /path/to/stdlib -I /path/to/deps
```

## Testing Strategy

- **Lexer tests**: token-level snapshot tests for each token kind and edge case
- **Parser tests**: snapshot tests comparing parsed AST against expected structure
- **Resolution tests**: KerML snippets exercising name resolution scenarios (local, inherited, imported, chained)
- **Type checking tests**: specialization validity, conjugation, redefinition
- **Integration tests**: end-to-end `.kerml` files → expected diagnostics or serialized output
- **Error recovery tests**: malformed input → verify multiple diagnostics collected

## References

- [KerML 1.0 Beta 2 Specification (PDF)](https://www.omg.org/spec/KerML/1.0/PDF)
- [SysML v2 Release Repository](https://github.com/Systems-Modeling/SysML-v2-Release)
- [Understanding KerML and SysML v2](https://sim4edu.com/reading/kerml-sysml/)
- [Semantic Analysis of KerML (ER 2024)](https://link.springer.com/content/pdf/10.1007/978-3-031-75872-0_8.pdf)
