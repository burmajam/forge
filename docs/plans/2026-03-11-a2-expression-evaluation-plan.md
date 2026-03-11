# A2: Expression Evaluation — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace eager u64 multiplicity evaluation with symbolic `MultBound` that preserves feature references through resolve/validate/serialize.

**Architecture:** Replace `Bound` enum with `MultBound { Exact(u64), Unbounded, Ref(NameRef) }` in HIR. Feature references in multiplicity bounds are stored as unresolved `NameRef`s during lowering, resolved in the resolve pass (same pattern as specializations), and preserved symbolically for validation and serialization.

**Tech Stack:** Rust, kermlc workspace (kermlc_hir, kermlc_parser, kermlc_resolve, kermlc_validate, kermlc_serial_json)

**Design doc:** `docs/plans/2026-03-11-a2-expression-evaluation-design.md`

---

### Task 1: Replace `Bound` with `MultBound` in HIR types

**Files:**
- Modify: `crates/kermlc_hir/src/types.rs:59-72`

**Step 1: Write the failing test**

Add to `crates/kermlc_hir/src/types.rs` in the `#[cfg(test)] mod tests` block (line 229):

```rust
#[test]
fn mult_bound_ref_is_not_copy() {
    // MultBound::Ref contains NameRef (has Vec), so MultBound must be Clone, not Copy
    let nr = NameRef::unresolved(vec![], Span::new(crate::FileId::from_raw(0), 0, 0));
    let bound = MultBound::Ref(nr);
    let _cloned = bound.clone();
}
```

This imports `MultBound` which doesn't exist yet.

**Step 2: Run test to verify it fails**

Run: `cargo test -p kermlc_hir -- mult_bound_ref_is_not_copy`
Expected: FAIL — `MultBound` not found.

**Step 3: Write minimal implementation**

In `crates/kermlc_hir/src/types.rs`, replace lines 59-72:

```rust
// OLD:
// #[derive(Clone, Debug)]
// pub struct HirMultiplicity {
//     pub lower: u64,
//     pub upper: Bound,
//     pub span: Span,
// }
//
// #[derive(Clone, Copy, Debug, PartialEq, Eq)]
// pub enum Bound {
//     Exact(u64),
//     Unbounded,
// }

// NEW:
/// A multiplicity bound: concrete value, unbounded (*), or symbolic feature reference.
#[derive(Clone, Debug)]
pub enum MultBound {
    Exact(u64),
    Unbounded,
    Ref(NameRef),
}

/// Multiplicity bounds in the HIR.
#[derive(Clone, Debug)]
pub struct HirMultiplicity {
    pub lower: MultBound,
    pub upper: MultBound,
    pub span: Span,
}
```

Delete the old `Bound` enum entirely. Update the `pub use` in `crates/kermlc_hir/src/lib.rs` — remove `Bound` export, add `MultBound` export.

**Step 4: Run test to verify it passes**

Run: `cargo test -p kermlc_hir -- mult_bound_ref_is_not_copy`
Expected: PASS

**Step 5: Do NOT commit yet** — downstream crates won't compile until updated.

---

### Task 2: Update lowering to produce `MultBound`

**Files:**
- Modify: `crates/kermlc_hir/src/lower.rs:171-243`

**Step 1: Write the failing test**

Add to `crates/kermlc_hir/src/lower.rs` tests (after line 343):

```rust
#[test]
fn lower_multiplicity_with_feature_ref() {
    let (model, interner, sink) =
        lower("package P { type T { feature n : T; feature x : T [1..n]; } }");
    assert!(!sink.has_errors(), "errors: {:?}", sink.diagnostics());

    let pkg = &model.defs[model.roots[0]];
    let ty = &model.defs[pkg.children[0]];
    let x = &model.defs[ty.children[1]];
    let mult = x.multiplicity.as_ref().expect("x should have multiplicity");

    // Lower bound should be Exact(1)
    assert!(
        matches!(mult.lower, MultBound::Exact(1)),
        "lower should be Exact(1), got {:?}",
        mult.lower
    );

    // Upper bound should be Ref (unresolved)
    assert!(
        matches!(mult.upper, MultBound::Ref(_)),
        "upper should be Ref, got {:?}",
        mult.upper
    );

    if let MultBound::Ref(ref name_ref) = mult.upper {
        assert_eq!(name_ref.resolution, ResolutionState::Unresolved);
        assert_eq!(interner.resolve(name_ref.segments[0]), "n");
    }
}

#[test]
fn lower_multiplicity_exact_unchanged() {
    let (model, _interner, sink) =
        lower("package P { type T { feature x : T [0..1]; } }");
    assert!(!sink.has_errors());

    let pkg = &model.defs[model.roots[0]];
    let ty = &model.defs[pkg.children[0]];
    let feat = &model.defs[ty.children[0]];
    let mult = feat.multiplicity.as_ref().unwrap();
    assert!(matches!(mult.lower, MultBound::Exact(0)));
    assert!(matches!(mult.upper, MultBound::Exact(1)));
}

#[test]
fn lower_multiplicity_star_unchanged() {
    let (model, _interner, sink) =
        lower("package P { type T { feature x : T [0..*]; } }");
    assert!(!sink.has_errors());

    let pkg = &model.defs[model.roots[0]];
    let ty = &model.defs[pkg.children[0]];
    let feat = &model.defs[ty.children[0]];
    let mult = feat.multiplicity.as_ref().unwrap();
    assert!(matches!(mult.lower, MultBound::Exact(0)));
    assert!(matches!(mult.upper, MultBound::Unbounded));
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p kermlc_hir -- lower_multiplicity`
Expected: FAIL — old `lower_multiplicity` returns `u64` lower / `Bound` upper, not `MultBound`.

**Step 3: Write minimal implementation**

Replace `lower_multiplicity` and `eval_const_expr` in `crates/kermlc_hir/src/lower.rs` (lines 219-243):

```rust
fn lower_multiplicity(mult: &kermlc_ast::Multiplicity) -> HirMultiplicity {
    let lower = mult
        .lower
        .as_ref()
        .map(lower_expr_to_bound)
        .unwrap_or(MultBound::Exact(0));

    let upper = mult
        .upper
        .as_ref()
        .map(lower_expr_to_bound)
        .unwrap_or_else(|| lower.clone());

    HirMultiplicity {
        lower,
        upper,
        span: mult.span,
    }
}

fn lower_expr_to_bound(expr: &kermlc_ast::Expr) -> MultBound {
    match expr {
        kermlc_ast::Expr::IntLiteral { value, .. } => MultBound::Exact(*value),
        kermlc_ast::Expr::Star { .. } => MultBound::Unbounded,
        kermlc_ast::Expr::Name { name } => MultBound::Ref(NameRef::unresolved(
            name.segments.clone(),
            name.span,
        )),
        kermlc_ast::Expr::BinOp { .. } => MultBound::Exact(0),
    }
}
```

Note: `QualifiedName.segments` is already `Vec<SymbolId>`, so no conversion needed. The `&StringInterner` parameter from the design doc is not necessary.

Also update the existing test `lower_creates_feature_with_type_ref` (line 329) to use `MultBound` instead of old `Bound`:

```rust
// OLD:
assert_eq!(mult.lower, 0);
assert_eq!(mult.upper, Bound::Exact(1));

// NEW:
assert!(matches!(mult.lower, MultBound::Exact(0)));
assert!(matches!(mult.upper, MultBound::Exact(1)));
```

**Step 4: Run tests to verify they pass**

Run: `cargo test -p kermlc_hir`
Expected: PASS (all HIR tests)

**Step 5: Commit**

```bash
git add crates/kermlc_hir/src/types.rs crates/kermlc_hir/src/lower.rs
git commit -m "feat(hir): replace Bound with MultBound for symbolic multiplicity"
```

---

### Task 3: Fix kermlc_hir exports

**Files:**
- Modify: `crates/kermlc_hir/src/lib.rs`

**Step 1: Check current exports**

Read `crates/kermlc_hir/src/lib.rs` and find where `Bound` is exported. Replace with `MultBound`.

**Step 2: Update exports**

Remove `Bound` from the public API, add `MultBound`. The exact change depends on current `pub use` statements.

**Step 3: Verify HIR crate compiles**

Run: `cargo build -p kermlc_hir`
Expected: PASS

**Step 4: Do NOT commit yet** — downstream crates still reference `Bound`.

---

### Task 4: Update validation for `MultBound`

**Files:**
- Modify: `crates/kermlc_validate/src/validate.rs:1-2, 243-336`

**Step 1: Write the failing test**

Add to `crates/kermlc_validate/src/validate.rs` tests (after line 431):

```rust
#[test]
fn multiplicity_with_feature_ref_defers_validation() {
    // Feature ref in upper bound — should NOT produce a bounds error
    let (_model, sink) =
        compile_and_validate("package P { type T { feature n : T; feature x : T [5..n]; } }");
    let bound_errors: Vec<_> = sink
        .diagnostics()
        .iter()
        .filter(|d| d.message.contains("exceeds upper bound"))
        .collect();
    assert!(
        bound_errors.is_empty(),
        "symbolic bound should defer validation, got: {:?}",
        bound_errors
    );
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p kermlc_validate -- multiplicity_with_feature_ref`
Expected: FAIL — compilation error because `Bound` no longer exists.

**Step 3: Write minimal implementation**

Update import at line 2:

```rust
// OLD:
use kermlc_hir::{Bound, DefId, DefKind, SemanticModel};
// NEW:
use kermlc_hir::{DefId, DefKind, MultBound, SemanticModel};
```

Update `validate_feature` multiplicity check (lines 243-257):

```rust
// OLD:
if let Some(mult) = &def.multiplicity {
    match mult.upper {
        Bound::Exact(upper) if mult.lower > upper => {
            sink.emit(
                Diagnostic::error(format!(
                    "multiplicity lower bound ({}) exceeds upper bound ({})",
                    mult.lower, upper
                ))
                .with_label(Label::primary(mult.span, "invalid multiplicity")),
            );
        }
        _ => {}
    }
}

// NEW:
if let Some(mult) = &def.multiplicity {
    if let (MultBound::Exact(lower), MultBound::Exact(upper)) =
        (&mult.lower, &mult.upper)
    {
        if lower > upper {
            sink.emit(
                Diagnostic::error(format!(
                    "multiplicity lower bound ({}) exceeds upper bound ({})",
                    lower, upper
                ))
                .with_label(Label::primary(mult.span, "invalid multiplicity")),
            );
        }
    }
}
```

Update `validate_redefinition_multiplicity` (lines 281-336):

```rust
fn validate_redefinition_multiplicity(
    model: &SemanticModel,
    interner: &StringInterner,
    redefining: DefId,
    inherited: DefId,
    sink: &mut DiagnosticSink,
) {
    let redef = &model.defs[redefining];
    let orig = &model.defs[inherited];

    let (redef_mult, orig_mult) = match (&redef.multiplicity, &orig.multiplicity) {
        (Some(r), Some(o)) => (r, o),
        _ => return,
    };

    // Only compare when both bounds are concrete
    if let (MultBound::Exact(redef_lo), MultBound::Exact(orig_lo)) =
        (&redef_mult.lower, &orig_mult.lower)
    {
        if redef_lo < orig_lo {
            let name = interner.resolve(redef.name);
            sink.emit(
                Diagnostic::warning(format!(
                    "redefined feature `{}` narrows lower multiplicity bound from {} to {}",
                    name, orig_lo, redef_lo
                ))
                .with_label(Label::primary(redef_mult.span, "redefined here"))
                .with_label(Label::secondary(orig_mult.span, "original multiplicity")),
            );
        }
    }

    match (&redef_mult.upper, &orig_mult.upper) {
        (MultBound::Unbounded, MultBound::Exact(_)) => {
            let name = interner.resolve(redef.name);
            sink.emit(
                Diagnostic::error(format!(
                    "redefined feature `{}` widens upper multiplicity bound to unbounded",
                    name
                ))
                .with_label(Label::primary(redef_mult.span, "widens multiplicity"))
                .with_label(Label::secondary(orig_mult.span, "original multiplicity")),
            );
        }
        (MultBound::Exact(r), MultBound::Exact(o)) if r > o => {
            let name = interner.resolve(redef.name);
            sink.emit(
                Diagnostic::error(format!(
                    "redefined feature `{}` widens upper multiplicity bound from {} to {}",
                    name, o, r
                ))
                .with_label(Label::primary(redef_mult.span, "widens multiplicity"))
                .with_label(Label::secondary(orig_mult.span, "original multiplicity")),
            );
        }
        _ => {} // Ref or matching — defer or ok
    }
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test -p kermlc_validate`
Expected: PASS (all validation tests including the new one)

**Step 5: Commit**

```bash
git add crates/kermlc_validate/src/validate.rs
git commit -m "feat(validate): update multiplicity validation for MultBound"
```

---

### Task 5: Update JSON-LD serialization for `MultBound`

**Files:**
- Modify: `crates/kermlc_serial_json/src/serialize.rs:153-164`

**Step 1: Write the failing test**

Add to `crates/kermlc_serial_json/src/serialize.rs` tests (after line 301):

```rust
#[test]
fn serialize_multiplicity_with_feature_ref() {
    let json = compile_and_serialize(
        "package P { type T { feature n : T; feature x : T [1..n]; } }",
    );
    let value: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap();

    let x_elem = value.iter().find(|e| e["name"] == "x").unwrap();
    let mult = &x_elem["ownedMultiplicity"];
    assert_eq!(mult["@type"], "MultiplicityRange");
    assert_eq!(mult["lowerBound"], 1);
    // Upper bound should be a FeatureReferenceExpression, not a number
    assert_eq!(
        mult["upperBound"]["@type"], "FeatureReferenceExpression",
        "upper bound should serialize as FeatureReferenceExpression"
    );
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p kermlc_serial_json -- serialize_multiplicity_with_feature_ref`
Expected: FAIL — compilation error because `Bound` no longer exists.

**Step 3: Write minimal implementation**

Replace multiplicity serialization (lines 153-164):

```rust
// Add multiplicity
if let Some(mult) = &def.multiplicity {
    element["ownedMultiplicity"] = json!({
        "@type": "MultiplicityRange",
        "lowerBound": mult_bound_to_json(&mult.lower, model, interner),
        "upperBound": mult_bound_to_json(&mult.upper, model, interner),
    });
}
```

Add helper function (before `build_elements` or after it, outside the function):

```rust
fn mult_bound_to_json(
    bound: &kermlc_hir::MultBound,
    model: &SemanticModel,
    interner: &StringInterner,
) -> Value {
    match bound {
        kermlc_hir::MultBound::Exact(n) => json!(n),
        kermlc_hir::MultBound::Unbounded => json!("*"),
        kermlc_hir::MultBound::Ref(name_ref) => match name_ref.resolved_def() {
            Some(def_id) => {
                let name = interner.resolve(model.defs[def_id].name);
                json!({
                    "@type": "FeatureReferenceExpression",
                    "reference": name,
                })
            }
            None => json!({
                "@type": "FeatureReferenceExpression",
                "reference": null,
            }),
        },
    }
}
```

Also remove the `kermlc_hir::Bound` import at the top if present (currently uses `Bound` implicitly via the match).

**Step 4: Run tests to verify they pass**

Run: `cargo test -p kermlc_serial_json`
Expected: PASS

**Step 5: Commit**

```bash
git add crates/kermlc_serial_json/src/serialize.rs
git commit -m "feat(serial): serialize MultBound feature refs in JSON-LD"
```

---

### Task 6: Simplify parser `parse_multiplicity`

**Files:**
- Modify: `crates/kermlc_parser/src/parser.rs:586-622`

**Step 1: Write the failing tests**

Add to `crates/kermlc_parser/src/parser.rs` tests:

```rust
#[test]
fn parse_multiplicity_name_exact() {
    let (result, interner, sink) =
        parse("package P { type T { feature x : T [n]; } }");
    assert!(!sink.has_errors(), "errors: {:?}", sink.diagnostics());
    let pkg = &result.packages[result.source_file.packages[0]];
    let feat = &result.features[match &pkg.members[0] {
        crate::Member::Type(id) => match &result.types[*id].members[0] {
            crate::Member::Feature(fid) => *fid,
            _ => panic!("expected feature"),
        },
        _ => panic!("expected type"),
    }];
    let mult = feat.multiplicity.as_ref().expect("should have multiplicity");
    assert!(mult.lower.is_none(), "exact mult should have no lower");
    assert!(matches!(mult.upper, Some(Expr::Name { .. })));
}

#[test]
fn parse_multiplicity_name_range() {
    let (result, _interner, sink) =
        parse("package P { type T { feature x : T [a..b]; } }");
    assert!(!sink.has_errors(), "errors: {:?}", sink.diagnostics());
    let pkg = &result.packages[result.source_file.packages[0]];
    let feat = &result.features[match &pkg.members[0] {
        crate::Member::Type(id) => match &result.types[*id].members[0] {
            crate::Member::Feature(fid) => *fid,
            _ => panic!("expected feature"),
        },
        _ => panic!("expected type"),
    }];
    let mult = feat.multiplicity.as_ref().expect("should have multiplicity");
    assert!(matches!(mult.lower, Some(Expr::Name { .. })));
    assert!(matches!(mult.upper, Some(Expr::Name { .. })));
}

#[test]
fn parse_multiplicity_int_to_name() {
    let (_result, _interner, sink) =
        parse("package P { type T { feature x : T [1..n]; } }");
    assert!(!sink.has_errors(), "errors: {:?}", sink.diagnostics());
}

#[test]
fn parse_multiplicity_name_to_star() {
    let (_result, _interner, sink) =
        parse("package P { type T { feature x : T [n..*]; } }");
    assert!(!sink.has_errors(), "errors: {:?}", sink.diagnostics());
}

#[test]
fn parse_multiplicity_qualified_name() {
    let (_result, _interner, sink) =
        parse("package P { type T { feature x : T [Pkg::count]; } }");
    assert!(!sink.has_errors(), "errors: {:?}", sink.diagnostics());
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p kermlc_parser -- parse_multiplicity_name`
Expected: FAIL — current parser rejects identifiers in multiplicity position.

**Step 3: Write minimal implementation**

Replace `parse_multiplicity` (lines 586-622):

```rust
fn parse_multiplicity(&mut self) -> Option<Multiplicity> {
    let start = self.current_span();
    self.expect(TokenKind::LBracket)?;

    let mut lower = None;
    let mut upper = None;

    let first = self.parse_expr_atom()?;

    if self.at(TokenKind::DotDot) {
        self.bump();
        lower = Some(first);
        upper = self.parse_expr_atom();
    } else {
        upper = Some(first);
    }

    let end = self.current_span();
    self.expect(TokenKind::RBracket);

    Some(Multiplicity {
        lower,
        upper,
        span: Span::new(start.file, start.start, end.end),
    })
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test -p kermlc_parser`
Expected: PASS (all parser tests)

**Step 5: Commit**

```bash
git add crates/kermlc_parser/src/parser.rs
git commit -m "feat(parser): accept feature refs in multiplicity bounds"
```

---

### Task 7: Add multiplicity resolution to resolve pass

**Files:**
- Modify: `crates/kermlc_resolve/src/resolve.rs:8-34, 37-99`

**Step 1: Write the failing test**

Add to `crates/kermlc_resolve/src/resolve.rs` tests (after line 493):

```rust
#[test]
fn resolve_multiplicity_feature_ref() {
    let (mut model, interner, mut sink) = parse_and_lower(
        "package P { type T { feature n : T; feature x : T [1..n]; } }",
    );
    assert!(!sink.has_errors(), "parse errors: {:?}", sink.diagnostics());

    resolve_pass(&mut model, &interner, &mut sink);

    let pkg = model.roots[0];
    let ty = model.defs[pkg].children[0];
    let x_id = model.defs[ty].children[1]; // second feature
    let mult = model.defs[x_id]
        .multiplicity
        .as_ref()
        .expect("x should have multiplicity");

    if let kermlc_hir::MultBound::Ref(ref name_ref) = mult.upper {
        assert!(
            name_ref.is_resolved(),
            "multiplicity ref 'n' should resolve to the feature"
        );
    } else {
        panic!("upper bound should be MultBound::Ref, got {:?}", mult.upper);
    }
}

#[test]
fn unresolved_multiplicity_ref_produces_error() {
    let (mut model, interner, mut sink) = parse_and_lower(
        "package P { type T { feature x : T [1..noSuchFeature]; } }",
    );

    resolve_pass(&mut model, &interner, &mut sink);
    emit_unresolved_errors(&model, &interner, &mut sink);

    assert!(
        sink.has_errors(),
        "unresolved multiplicity ref should produce error"
    );
    let has_mult_error = sink
        .diagnostics()
        .iter()
        .any(|d| d.message.contains("multiplicity bound"));
    assert!(
        has_mult_error,
        "error should mention 'multiplicity bound': {:?}",
        sink.diagnostics()
    );
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p kermlc_resolve -- multiplicity`
Expected: FAIL — no resolution logic for multiplicity refs.

**Step 3: Write minimal implementation**

Add `resolve_multiplicity_refs_for` function (after `resolve_chains_for`, around line 312):

```rust
fn resolve_multiplicity_refs_for(model: &mut SemanticModel, def_id: DefId) -> bool {
    let mut changed = false;

    if let Some(ref mult) = model.defs[def_id].multiplicity {
        if let kermlc_hir::MultBound::Ref(ref name_ref) = mult.lower {
            if name_ref.resolution == ResolutionState::Unresolved {
                let segments = name_ref.segments.clone();
                if let Some(resolved) = try_resolve_name(model, def_id, &segments) {
                    if let kermlc_hir::MultBound::Ref(ref mut r) =
                        model.defs[def_id].multiplicity.as_mut().unwrap().lower
                    {
                        r.resolution = ResolutionState::Resolved(resolved);
                        changed = true;
                    }
                }
            }
        }
    }

    if let Some(ref mult) = model.defs[def_id].multiplicity {
        if let kermlc_hir::MultBound::Ref(ref name_ref) = mult.upper {
            if name_ref.resolution == ResolutionState::Unresolved {
                let segments = name_ref.segments.clone();
                if let Some(resolved) = try_resolve_name(model, def_id, &segments) {
                    if let kermlc_hir::MultBound::Ref(ref mut r) =
                        model.defs[def_id].multiplicity.as_mut().unwrap().upper
                    {
                        r.resolution = ResolutionState::Resolved(resolved);
                        changed = true;
                    }
                }
            }
        }
    }

    changed
}
```

Add call in `resolve_pass` (after line 30, before closing brace of the loop):

```rust
        // Resolve multiplicity refs
        changed |= resolve_multiplicity_refs_for(model, def_id);
```

Add multiplicity check in `emit_unresolved_errors` (after chain_segments block, line 98):

```rust
        if let Some(ref mult) = def.multiplicity {
            for bound in [&mult.lower, &mult.upper] {
                if let kermlc_hir::MultBound::Ref(ref r) = bound {
                    if r.resolution == ResolutionState::Unresolved {
                        let name_str = segments_to_string(&r.segments, interner);
                        sink.emit(
                            Diagnostic::error(format!(
                                "unresolved multiplicity bound `{}`",
                                name_str
                            ))
                            .with_label(Label::primary(r.span, "not found")),
                        );
                    }
                }
            }
        }
```

**Step 4: Run tests to verify they pass**

Run: `cargo test -p kermlc_resolve`
Expected: PASS

**Step 5: Commit**

```bash
git add crates/kermlc_resolve/src/resolve.rs
git commit -m "feat(resolve): resolve feature refs in multiplicity bounds"
```

---

### Task 8: Add integration test fixtures

**Files:**
- Create: `crates/kermlc/tests/fixtures/valid/multiplicity_feature_ref.kerml`
- Create: `crates/kermlc/tests/fixtures/invalid/multiplicity_unresolved_ref.kerml`
- Modify: `crates/kermlc/tests/integration.rs`

**Step 1: Create valid fixture**

Write `crates/kermlc/tests/fixtures/valid/multiplicity_feature_ref.kerml`:

```kerml
package MultRef {
    type Connection {
        feature portCount : Anything [1];
        feature ports : Anything [1..portCount];
    }
}
```

**Step 2: Create invalid fixture**

Write `crates/kermlc/tests/fixtures/invalid/multiplicity_unresolved_ref.kerml`:

```kerml
package Bad {
    type T {
        feature f : Anything [1..noSuchFeature];
    }
}
```

**Step 3: Add integration tests**

Add to `crates/kermlc/tests/integration.rs` (after `valid_direction` test):

```rust
#[test]
fn valid_multiplicity_feature_ref() {
    let result = compile_file(&fixtures_dir().join("valid/multiplicity_feature_ref.kerml"));
    assert!(
        !result.sink.has_errors(),
        "Errors in multiplicity_feature_ref.kerml: {:?}",
        result.sink.diagnostics()
    );

    // Verify the multiplicity ref resolved
    let pkg = result.model.roots[0];
    let conn_id = result.model.defs[pkg].children[0];
    let ports_id = result.model.defs[conn_id].children[1];
    let mult = result.model.defs[ports_id]
        .multiplicity
        .as_ref()
        .expect("ports should have multiplicity");

    assert!(
        matches!(mult.lower, kermlc_hir::MultBound::Exact(1)),
        "lower should be Exact(1)"
    );
    if let kermlc_hir::MultBound::Ref(ref name_ref) = mult.upper {
        assert!(
            name_ref.is_resolved(),
            "upper bound 'portCount' should be resolved"
        );
    } else {
        panic!("upper bound should be MultBound::Ref");
    }
}
```

Add invalid test (after `invalid_feature_conjugates_type` test):

```rust
#[test]
fn invalid_multiplicity_unresolved_ref() {
    let result =
        compile_file(&fixtures_dir().join("invalid/multiplicity_unresolved_ref.kerml"));
    assert!(
        result.sink.has_errors(),
        "Expected errors for unresolved multiplicity ref"
    );
    let has_mult_error = result
        .sink
        .diagnostics()
        .iter()
        .any(|d| d.message.contains("multiplicity bound"));
    assert!(
        has_mult_error,
        "should have multiplicity bound error: {:?}",
        result.sink.diagnostics()
    );
}
```

**Step 4: Run all integration tests**

Run: `cargo test -p kermlc -- integration`
Expected: PASS (all integration tests)

**Step 5: Commit**

```bash
git add crates/kermlc/tests/fixtures/valid/multiplicity_feature_ref.kerml \
       crates/kermlc/tests/fixtures/invalid/multiplicity_unresolved_ref.kerml \
       crates/kermlc/tests/integration.rs
git commit -m "test: add integration tests for multiplicity feature refs"
```

---

### Task 9: Full workspace verification

**Step 1: Run full test suite**

Run: `cargo test`
Expected: ALL PASS

**Step 2: Run clippy**

Run: `cargo clippy --all-targets -- -D warnings`
Expected: No warnings

**Step 3: Run format check**

Run: `cargo fmt --check`
Expected: No formatting issues

**Step 4: Fix any issues found in steps 1-3**

If any test, clippy warning, or format issue exists, fix it.

**Step 5: Final commit if fixes needed**

```bash
git add -A
git commit -m "chore: fix clippy and formatting for A2"
```

---

### Task 10: Update progress tracker

**Files:**
- Modify: `docs/plans/progress.md`
- Modify: `/Users/mjaric/.claude/projects/-Users-mjaric-prj-mjaric-forge/memory/progress.md`

**Step 1: Mark A2 complete in both trackers**

Change:
```markdown
- [ ] A2: Expression evaluation — Star, Name, BinOp in multiplicity
```
To:
```markdown
- [x] A2: Expression evaluation — symbolic MultBound (Star, IntLiteral, FeatureRef)
```

**Step 2: Commit**

```bash
git add docs/plans/progress.md
git commit -m "docs: mark A2 expression evaluation as complete"
```
