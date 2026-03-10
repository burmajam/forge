# A1: Full Conjugation — Design

## Goal

Implement full type-level conjugation with input/output direction flipping. When type B conjugates type A, B inherits A's features with `in` ↔ `out` directions flipped. `inout` and undirected features are unchanged.

## Scope

**In scope (A1):**
- Feature direction parsing (`in`, `out`, `inout` modifiers)
- Direction storage in AST and HIR
- Direction flipping during conjugation inheritance in type checking
- Inheritance origin tagging (`Specialization` vs `Conjugation`)
- Validation of direction consistency
- Serialization of feature directions
- Populating `TypeInfo::conjugate_of`

**Out of scope (future tasks):**
- A1a: Named conjugation declarations (`conjugation c1 conjugate X conjugates Y;`)
- A1b: Feature-level conjugation (`feature g ~ B::f;`)
- A1c: Inline conjugated type refs (`feature port : ~FuelPort;`, anonymous type synthesis)

## Approach

Two vertical slices, each independently testable:

1. **Slice 1 — Feature directions:** Parse `in`/`out`/`inout` → AST → HIR → serialization
2. **Slice 2 — Conjugation flip:** Inheritance tagging → direction flipping in typeck → validation → serialization

## Data Model

### Feature Direction

```rust
// kermlc_hir
enum FeatureDirection {
    In,
    Out,
    InOut,
}
```

Absence of direction represented by `Option<FeatureDirection> = None`.

### Inheritance Origin

```rust
// kermlc_hir
enum InheritanceKind {
    Specialization,
    Conjugation,
}
```

### Inherited Feature

```rust
// kermlc_hir
struct InheritedFeature {
    def_id: DefId,
    kind: InheritanceKind,
    direction_override: Option<FeatureDirection>,
}
```

- `direction_override = None` for specialization (use original feature's direction)
- `direction_override = Some(flipped)` for conjugation

### Direction Flip Rule

| Original | Conjugated |
|----------|------------|
| `in`     | `out`      |
| `out`    | `in`       |
| `inout`  | `inout`    |
| (none)   | (none)     |

```rust
fn conjugate_direction(
    dir: Option<FeatureDirection>,
) -> Option<FeatureDirection> {
    match dir {
        Some(FeatureDirection::In) => Some(FeatureDirection::Out),
        Some(FeatureDirection::Out) => Some(FeatureDirection::In),
        Some(FeatureDirection::InOut) => Some(FeatureDirection::InOut),
        None => None,
    }
}
```

## Parser Changes

### New Tokens

Add `In`, `Out`, `InOut` to `TokenKind`.

### Grammar

```
FeatureDecl = Direction? 'feature' Name ':' TypeRef FeatureChain? Multiplicity? ';'
Direction   = 'in' | 'out' | 'inout'
```

Direction modifier precedes the `feature` keyword: `in feature f : T;`

### AST

`FeatureDecl.direction: Option<FeatureDirection>` — new field.

### Lowering

Direct mapping: `AST FeatureDecl.direction` → `HIR Def.direction`.

## Type Checking Changes

When processing conjugation for type B that conjugates type A:

1. Collect A's direct features
2. Collect A's inherited features
3. For each feature, create `InheritedFeature` with:
   - `kind: InheritanceKind::Conjugation`
   - `direction_override: conjugate_direction(feature.effective_direction())`
4. Store in B's inherited features
5. Set `TypeInfo::conjugate_of = Some(a_def_id)`

Chained conjugation (B ~ A, C ~ B) works correctly: C flips B's effective directions, which are already flipped from A. Result: C has same directions as A.

## Validation Changes

1. **Direction consistency on redefinition** — redefined feature's direction must match original (conjugation-flipped features exempt)
2. **Conjugation target must have features** — warning if conjugating a type with no features (no effect)
3. **Existing rule preserved** — conjugation target must be a type

## Serialization Changes

1. Emit `"direction"` field on feature elements (`"in"`, `"out"`, `"inout"`, omit if undirected)
2. Use `direction_override` for inherited features from conjugation
3. Conjugation element output unchanged

## Testing

### Slice 1 — Feature Directions

- Lexer: tokenize `in`, `out`, `inout`
- Parser: all direction variants + plain feature
- Roundtrip: parse → lower → serialize, verify `"direction"` in JSON-LD
- Integration fixture: `direction.kerml`

### Slice 2 — Conjugation Flip

- Unit: type A with `in`/`out`/`inout`/undirected features, type B ~ A → verify flipped directions
- Inheritance tagging: verify `Conjugation` vs `Specialization` tags
- Chained conjugation: A → B ~ A → C ~ B → verify C has original directions
- Validation: direction consistency, conjugation target checks
- Serialization: JSON-LD shows flipped directions
- Integration fixture: update `conjugation.kerml`
- Mutation testing: `cargo-mutants` on `conjugate_direction`

## Spec References

- KerML 1.0 Beta 2, Section 7.3.4 — Conjugation
- Vendor examples: `Simple Tests/Conjugation.kerml`, `Simple Tests/Types.kerml`, `Simple Tests/Features.kerml`
