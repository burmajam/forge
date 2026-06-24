# kermlc Progress Tracker

This tasks are related to the design in [2026-02-15-kermlc-compiler-design.md](./2026-02-15-kermlc-compiler-design.md)

Last updated: 2026-06-23 (harpoon architecture cleanup merged, PR #1)

> Crate naming: the shared engine carries the `harpoon_*` prefix behind the `harpoon` facade;
> the KerML front-end keeps `kermlc_*` (lexer, parser, ast, lower). See ADR-0001 / `CONTEXT.md`.

## Milestone 1 — Complete

All items shipped, pipeline works end-to-end.

- [x] String interning + arenas (harpoon_intern)
- [x] Diagnostics + SourceMap + Span (harpoon_diagnostics)
- [x] Lexer — 28 token kinds (kermlc_lexer)
- [x] AST node types (kermlc_ast)
- [x] Recursive descent parser with error recovery (kermlc_parser)
- [x] HIR types (harpoon_hir) + AST lowering (kermlc_lower)
- [x] Minimal stdlib — 6 hardcoded types (harpoon_hir)
- [x] Implicit specialization (harpoon_hir) — d43e2b4
- [x] Name resolution — 5-strategy, fixpoint (harpoon_resolve)
- [x] Cycle detection — 3-color DFS (harpoon_resolve)
- [x] Type checking — inheritance, simplified conjugation (harpoon_typeck)
- [x] Validation — multiplicity, redefinition, targets (harpoon_validate)
- [x] JSON-LD serialization (harpoon_serial_json)
- [x] CLI: check + compile commands (kermlc)
- [x] Integration tests — 7 fixtures (kermlc)

## Architecture cleanup — Complete (2026-06-23, PR #1, harpoon `289c1b2`)

Behavior-neutral; 129 tests, 0 ignored, clippy `-D warnings` clean.

- [x] `harpoon` facade crate + deep `harpoon::compile()` (fixpoint + emit_unresolved + cycles + validate)
- [x] `kermlc::compile_source` (lib.rs) single driver entry; thin `main.rs`; `pipeline.rs` deleted
- [x] Extract `kermlc_lower`; `harpoon_hir` AST-free; owns `FeatureDirection`/`Visibility`
- [x] Rename engine + foundation crates `kermlc_* → harpoon_*`
- [x] Break resolve↔typeck dev-dep cycle (fixpoint tests → `kermlc` integration)
- [x] Records: ADR-0001, ADR-0002, `CONTEXT.md`

## Post-Milestone 1 — In Progress

### A. Compiler Depth (deepen existing pipeline)

- [x] A1: Full conjugation — input/output direction flipping (type-level, explicit form)
- [x] A1a: Named conjugation declarations — `conjugation c1 conjugate X conjugates Y;`
- [x] A1b: Feature-level conjugation — `feature g ~ B::f;`
- [x] A1c: Inline conjugated type refs — `feature port : ~FuelPort;` (anonymous type synthesis)
- [x] A2: Expression evaluation — symbolic MultBound (Star, IntLiteral, FeatureRef)
- [x] A3: Feature chaining resolution — type-directed chain walking (a.b.c)
- [ ] A4: Diamond inheritance + Redefinition + Subsetting (depends on: Membership layer)
  - [ ] A4a: Parse `redefines`/`:>>` (inline + standalone `redefinition` declarations)
  - [ ] A4b: Parse `subsets`/`:>`, `references`/`::>`, `crosses` (all feature relationship variants)
  - [ ] A4c: `FeatureRelationship { kind, target }` in HIR — unified storage for all variants
  - [ ] A4d: Resolve feature relationship targets in resolve pass
  - [ ] A4e: Implicit redefinition — name-based shadowing (owned member same name as inherited)
  - [ ] A4f: `removeRedefinedFeatures` — filter inherited memberships (explicit + implicit)
  - [ ] A4g: Diamond inheritance — ordering-independent dedup via MembershipId
  - **Deferred to later:**
  - [ ] A4-D1: Deep type-compatibility validation for redefinition (redefining must subtype redefined)
  - [ ] A4-D2: Multiplicity compatibility validation for subsetting
  - [ ] A4-D3: Cross-subsetting semantics (`crosses` beyond parsing + storage)
  - [ ] A4-D4: Reference subsetting semantics (`references`/`::>` beyond parsing + storage)
- [x] A5: Visibility (public/protected/private) — grammar + semantics + Membership layer
- [ ] A6: DiagnosticCode + Suggestions — E0001 codes, "did you mean?" hints

### B. Compiler Breadth (grow grammar + capabilities)

- [ ] B1: Multi-file compilation — cross-file references
- [ ] B2: Stdlib from files — load real Kernel Semantic Library from disk
- [ ] B3: Grow grammar beyond milestone 1 subset
  - [ ] B3-B: Dot-chains in subsetting/specialization contexts (`subset g.g subsets b.f.a;`)
  - [ ] B3-C: Dot-chains in expression contexts (`feature g = f.a;`)

### C. Serialization

- [ ] C1: XMI serialization backend (harpoon_serial_xmi) — second adapter makes the serialization seam real

### D. Project Tooling

- [ ] D1: kermlc_project — KerML.toml manifest, dependency resolution
- [ ] D2: forge CLI — new, check, build, add, publish, fetch
- [ ] D3: KPAR archive support — OMG normative ZIP format
- [ ] D4: Registry client — Systems Modeling API integration

### E. Architecture follow-ups (deferred from the 2026-06-23 review; candidate #1 done)

- [ ] #2: Deepen the resolve crate's own interface — *largely addressed*: `harpoon::compile()` now
      owns the call order, so callers no longer choreograph `emit_unresolved_errors` /
      `detect_specialization_cycles`. Optional remaining work: fold them behind a single
      `harpoon_resolve` entry point.
- [ ] #3: Validation rules as small testable modules (`(model, def) -> Vec<Diagnostic>` + thin driver)
- [ ] #4: Diagnostics catalog — centralize messages + error codes (merge with A6)
- [ ] #5: `SemanticModel` mutation interface — guarded state-transitions only (not blanket getters)

### F. Open architecture decisions (ADR-0002)

- [ ] Incremental / partial recompilation
- [ ] Stdlib lookup: implicit **sysroot** for kernel/systems libraries vs `-I` for user libraries (ties to B2)
- [ ] Language-parameterized implicit specialization (KerML vs SysML target different libraries; needed for B2 + SysML)
- [ ] In-memory model → JSON-LD/XMI conversion on demand (model is primary, serialization is output)

### G. Future arcs

- [ ] SysML v2 front-end: `sysml_lexer` / `sysml_parser` / `sysml_ast` / `sysml_lower` → same `harpoon` engine (desugaring 1→N)
- [ ] Simulator: in-memory model → evaluation engine (expressions, occurrences/time/succession, concrete values)
