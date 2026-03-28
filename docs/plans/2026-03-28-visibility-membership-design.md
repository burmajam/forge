# A5: Visibility + Membership Layer

Date: 2026-03-28
Status: Approved
Depends on: Milestone 1 (complete)
Prepares: A4 (diamond inheritance), B1 (multi-file), B2 (stdlib from files)

## Problem

The current HIR has no concept of visibility (`public`/`protected`/`private`) and models
parent-child ownership as bare `Vec<DefId>`. This loses relationship metadata that the
KerML spec attaches to Membership — visibility, membership kind (FeatureMembership vs
regular), and identity. Without visibility, inherited membership filtering is impossible,
blocking A4 (diamond inheritance with `removeRedefinedFeatures`).

## Decision: Membership as arena-allocated entity

Membership is a relationship between a namespace and a member element. In the spec it is
an Element with its own identity. We model it as an arena-allocated struct with `MembershipId`.

Why arena (not inline struct on Def):
- **Diamond inheritance dedup**: When type D specializes both B and C, and both inherit
  membership M1 from A, D collects M1 twice. Dedup by `MembershipId` is identity comparison.
  Inline structs require structural comparison — slower and loses identity.
- **Spec fidelity**: `removeRedefinedFeatures` and `visibilityOf(mem)` operate on Membership
  identity, not copies.
- **Serialization**: Memberships appear as distinct objects in JSON-LD output.

Why no indexes:
- Both LSP and runtime consumers do **local graph traversal** — jump to node by ID (O(1)),
  iterate edges of that node. No global search needed.
- Name lookup is linear scan within a single namespace (5-50 members typically).
- Indexes can be added later without API changes if profiling shows need (B2 stdlib scale).

## New types (kermlc_hir)

```rust
pub type MembershipId = Idx<Membership>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Visibility {
    Public,      // default for owned members
    Protected,   // inherited by subtypes, not visible externally
    Private,     // not inherited, not visible externally
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MembershipKind {
    Owning,   // namespace owns the element (package member, nested package, etc.)
    Feature,  // feature declared in type body (default for features in types)
    Member,   // `member` keyword — member but not "feature of" the type
}

pub struct Membership {
    pub visibility: Visibility,
    pub kind: MembershipKind,
    pub member_def: DefId,
    pub owning_namespace: DefId,
    pub span: Span,
}
```

## SemanticModel changes

```rust
pub struct SemanticModel {
    pub defs: Arena<Def>,
    pub memberships: Arena<Membership>,  // NEW
    pub roots: Vec<DefId>,
    // DELETED: type_infos, def_to_type
}
```

## Def changes

```rust
pub struct Def {
    // Identity
    pub name: SymbolId,
    pub kind: DefKind,
    pub span: Span,
    pub parent: Option<DefId>,

    // Membership-based (replaces children: Vec<DefId>)
    pub owned_memberships: Vec<MembershipId>,

    // Computed by typeck (replaces inherited_features: Vec<InheritedFeature>)
    pub inherited_memberships: Vec<MembershipId>,

    // Unchanged fields:
    pub specializations: Vec<NameRef>,
    pub conjugation: Option<NameRef>,
    pub type_ref: Option<NameRef>,
    pub chain_segments: Vec<NameRef>,
    pub chain_result: Option<DefId>,
    pub multiplicity: Option<HirMultiplicity>,
    pub direction: Option<FeatureDirection>,
    pub conjugation_decl: Option<(NameRef, NameRef)>,
    pub imports: Vec<Import>,
    pub type_checked: bool,
}
```

## Deleted types

- `InheritedFeature` — replaced by `inherited_memberships: Vec<MembershipId>`
- `InheritanceKind` — derivable from membership context
- `TypeInfo` + `type_infos` arena + `def_to_type` map — unused/replaced
- `conjugate_direction()` as storage helper — replaced by `direction_of()` compute

## Direction: computed, not stored

The current `InheritedFeature.direction_override` stores a pre-flipped direction.
This is replaced by an on-demand computation following the spec's `directionOfExcluding()`:

```rust
impl SemanticModel {
    pub fn direction_of(&self, feature: DefId, in_type: DefId) -> Option<FeatureDirection> {
        // 1. If feature owned by in_type → feature.direction
        // 2. Find supertype containing feature, recurse
        // 3. If in_type is conjugated → flip result
    }
}
```

## AST layer

New wrapper around existing `Member` enum:

```rust
pub struct MemberEntry {
    pub visibility: Option<Visibility>,  // None = unspecified (default applied in lowering)
    pub is_member_only: bool,            // `member` keyword present
    pub member: Member,
    pub span: Span,
}
```

`PackageDecl.members`, `TypeDecl.members`, `SourceFile.members` become `Vec<MemberEntry>`.

## Lexer additions

Four new keywords: `public`, `private`, `protected`, `member`.

## Import visibility

```rust
pub struct Import {
    pub path: NameRef,
    pub is_wildcard: bool,
    pub visibility: Visibility,  // default: Private (per spec 7.2.5.4)
    pub span: Span,
}
```

## SemanticModel convenience API

```rust
impl SemanticModel {
    /// Allocate membership, wire parent↔child relationship.
    pub fn add_member(
        &mut self, parent: DefId, child: DefId,
        visibility: Visibility, kind: MembershipKind, span: Span,
    ) -> MembershipId;

    /// Iterate owned child DefIds.
    pub fn children(&self, def: DefId) -> impl Iterator<Item = DefId>;

    /// Find owned child by name.
    pub fn find_child(&self, parent: DefId, name: SymbolId) -> Option<DefId>;

    /// Find member (owned + inherited), respecting minimum visibility.
    pub fn find_member(
        &self, ns: DefId, name: SymbolId, min_vis: Visibility,
    ) -> Option<DefId>;

    /// Compute direction of feature relative to type (spec directionOfExcluding).
    pub fn direction_of(&self, feature: DefId, in_type: DefId) -> Option<FeatureDirection>;
}
```

## Grammar (MemberPrefix)

Per KerML BNF:
```
MemberPrefix = ( visibility = VisibilityIndicator )?
VisibilityIndicator = 'public' | 'private' | 'protected'
```

Extended for our parser:
```
MemberPrefix = ( VisibilityIndicator )? ( 'member' )?
```

The parser reads MemberPrefix before dispatching to `parse_type_decl`, `parse_feature_decl`, etc.

## Visibility semantics (spec 7.2.5, 7.3.2.3)

| Visibility | Visible outside namespace? | Inherited by subtypes? |
|------------|---------------------------|----------------------|
| `public`   | Yes                       | Yes                  |
| `protected`| No                        | Yes                  |
| `private`  | No                        | No                   |

Default visibility: `public` for owned members, `private` for imports.

Inheritance rule (spec `inheritableMemberships`):
> A type inherits all non-private memberships of its supertypes.
> Private memberships are not inherited.

Name conflict rules (spec 7.2.5, 7.3.2.3):
- Inherited member names must be distinct from each other and from owned member names.
- An imported membership is hidden by an inherited membership with the same name.
- An imported membership is hidden by an owned membership with the same name.

## Pipeline impact

| Phase       | Change                                                         |
|-------------|----------------------------------------------------------------|
| **Lexer**   | +4 keywords (public, private, protected, member)               |
| **Parser**  | Parse MemberPrefix before each member; parse visibility on import |
| **AST**     | MemberEntry wrapper, Visibility enum                           |
| **Lowering**| Create Membership in arena per member; apply default visibility |
| **Resolve** | Visibility-aware find_child/find_member; import visibility filtering |
| **Typeck**  | Collect inherited MembershipIds; filter Private; dedup by MembershipId; direction_of() |
| **Validate**| Name conflict rules across visibility levels                   |
| **Serial**  | Emit Membership objects in JSON-LD; use direction_of() for inherited |

## A4 preparation

With this model, diamond inheritance dedup becomes:

```
D.inherited_memberships = dedup_by_membership_id(
    supertypes.flat_map(|s| s.non_private_memberships())
)
```

`removeRedefinedFeatures` filters from that set by checking redefinition relationships.
MembershipId identity makes dedup trivial.
