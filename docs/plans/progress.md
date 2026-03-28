# kermlc Progress Tracker

This tasks are related to the design in [2026-02-15-kermlc-compiler-design.md](./2026-02-15-kermlc-compiler-design.md)

Last updated: 2026-03-28 (A3 complete)

## Milestone 1 — Complete

All items shipped, pipeline works end-to-end.

- [x] String interning + arenas (kermlc_intern)
- [x] Diagnostics + SourceMap + Span (kermlc_diagnostics)
- [x] Lexer — 28 token kinds (kermlc_lexer)
- [x] AST node types (kermlc_ast)
- [x] Recursive descent parser with error recovery (kermlc_parser)
- [x] HIR types + AST lowering (kermlc_hir)
- [x] Minimal stdlib — 6 hardcoded types (kermlc_hir)
- [x] Implicit specialization (kermlc_hir) — d43e2b4
- [x] Name resolution — 5-strategy, fixpoint (kermlc_resolve)
- [x] Cycle detection — 3-color DFS (kermlc_resolve)
- [x] Type checking — inheritance, simplified conjugation (kermlc_typeck)
- [x] Validation — multiplicity, redefinition, targets (kermlc_validate)
- [x] JSON-LD serialization (kermlc_serial_json)
- [x] CLI: check + compile commands (kermlc)
- [x] Integration tests — 7 fixtures (kermlc)

## Post-Milestone 1 — In Progress

### A. Compiler Depth (deepen existing pipeline)

- [x] A1: Full conjugation — input/output direction flipping (type-level, explicit form)
- [x] A1a: Named conjugation declarations — `conjugation c1 conjugate X conjugates Y;`
- [x] A1b: Feature-level conjugation — `feature g ~ B::f;`
- [x] A1c: Inline conjugated type refs — `feature port : ~FuelPort;` (anonymous type synthesis)
- [x] A2: Expression evaluation — symbolic MultBound (Star, IntLiteral, FeatureRef)
- [x] A3: Feature chaining resolution — type-directed chain walking (a.b.c)
- [ ] A4: Diamond inheritance — ordering-independent membership dedup
- [ ] A5: Visibility (public/protected/private) — grammar + semantics
- [ ] A6: DiagnosticCode + Suggestions — E0001 codes, "did you mean?" hints

### B. Compiler Breadth (grow grammar + capabilities)

- [ ] B1: Multi-file compilation — cross-file references
- [ ] B2: Stdlib from files — load real Kernel Semantic Library from disk
- [ ] B3: Grow grammar beyond milestone 1 subset
  - [ ] B3-B: Dot-chains in subsetting/specialization contexts (`subset g.g subsets b.f.a;`)
  - [ ] B3-C: Dot-chains in expression contexts (`feature g = f.a;`)

### C. Serialization

- [ ] C1: XMI serialization backend (kermlc_serial_xmi)

### D. Project Tooling

- [ ] D1: kermlc_project — KerML.toml manifest, dependency resolution
- [ ] D2: forge CLI — new, check, build, add, publish, fetch
- [ ] D3: KPAR archive support — OMG normative ZIP format
- [ ] D4: Registry client — Systems Modeling API integration
