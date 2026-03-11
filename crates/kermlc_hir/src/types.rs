use kermlc_diagnostics::Span;
use kermlc_intern::{Arena, Idx, SymbolId};

pub use kermlc_ast::FeatureDirection;

/// A definition ID — typed index into the def arena.
pub type DefId = Idx<Def>;

/// A type info ID — typed index into the type info arena.
pub type TypeId = Idx<TypeInfo>;

/// What kind of definition this is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefKind {
    Package,
    Type,
    Feature,
    Conjugation,
}

/// Resolution state for a name reference.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResolutionState {
    Unresolved,
    InProgress,
    Resolved(DefId),
    Error,
}

/// A reference to a name that may or may not be resolved yet.
#[derive(Clone, Debug)]
pub struct NameRef {
    pub segments: Vec<SymbolId>,
    pub span: Span,
    pub resolution: ResolutionState,
}

impl NameRef {
    pub fn unresolved(segments: Vec<SymbolId>, span: Span) -> Self {
        Self {
            segments,
            span,
            resolution: ResolutionState::Unresolved,
        }
    }

    pub fn is_resolved(&self) -> bool {
        matches!(self.resolution, ResolutionState::Resolved(_))
    }

    pub fn resolved_def(&self) -> Option<DefId> {
        match self.resolution {
            ResolutionState::Resolved(id) => Some(id),
            _ => None,
        }
    }
}

/// Multiplicity bounds in the HIR.
#[derive(Clone, Debug)]
pub struct HirMultiplicity {
    pub lower: u64,
    pub upper: Bound,
    pub span: Span,
}

/// Upper bound for multiplicity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bound {
    Exact(u64),
    Unbounded,
}

/// How a feature was inherited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InheritanceKind {
    Specialization,
    Conjugation,
}

/// A feature inherited from a supertype or conjugate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InheritedFeature {
    pub def_id: DefId,
    pub kind: InheritanceKind,
    pub direction_override: Option<FeatureDirection>,
}

/// Flip a feature direction for conjugation: in↔out, inout stays, None stays.
pub fn conjugate_direction(dir: Option<FeatureDirection>) -> Option<FeatureDirection> {
    match dir {
        Some(FeatureDirection::In) => Some(FeatureDirection::Out),
        Some(FeatureDirection::Out) => Some(FeatureDirection::In),
        Some(FeatureDirection::InOut) => Some(FeatureDirection::InOut),
        None => None,
    }
}

/// A definition in the semantic model.
#[derive(Clone, Debug)]
pub struct Def {
    pub name: SymbolId,
    pub kind: DefKind,
    pub span: Span,
    pub parent: Option<DefId>,
    pub children: Vec<DefId>,
    /// For types: specialization targets (resolved or not)
    pub specializations: Vec<NameRef>,
    /// For types: conjugation target
    pub conjugation: Option<NameRef>,
    /// For features: typing reference
    pub type_ref: Option<NameRef>,
    /// For features: chain segments
    pub chain_segments: Vec<NameRef>,
    /// For features: multiplicity
    pub multiplicity: Option<HirMultiplicity>,
    /// For features: direction modifier (in, out, inout)
    pub direction: Option<FeatureDirection>,
    /// For conjugation declarations: (conjugated_type, original_type)
    pub conjugation_decl: Option<(NameRef, NameRef)>,
    /// Imports visible from this def's scope
    pub imports: Vec<Import>,
    /// Inherited features (populated by type checking)
    pub inherited_features: Vec<InheritedFeature>,
    /// Whether this def has been fully type-checked
    pub type_checked: bool,
}

impl Def {
    pub fn new(name: SymbolId, kind: DefKind, span: Span) -> Self {
        Self {
            name,
            kind,
            span,
            parent: None,
            children: Vec::new(),
            specializations: Vec::new(),
            conjugation: None,
            conjugation_decl: None,
            type_ref: None,
            chain_segments: Vec::new(),
            multiplicity: None,
            direction: None,
            imports: Vec::new(),
            inherited_features: Vec::new(),
            type_checked: false,
        }
    }
}

/// An import declaration in the HIR.
#[derive(Clone, Debug)]
pub struct Import {
    pub path: NameRef,
    pub is_wildcard: bool,
    pub span: Span,
}

/// Resolved type information for a def.
#[derive(Clone, Debug)]
pub struct TypeInfo {
    pub def: DefId,
    /// Direct supertypes (resolved DefIds)
    pub supertypes: Vec<DefId>,
    /// All features (own + inherited), as DefIds
    pub all_features: Vec<DefId>,
    /// If this type is a conjugation of another
    pub conjugate_of: Option<DefId>,
}

/// The top-level semantic model.
pub struct SemanticModel {
    pub defs: Arena<Def>,
    pub type_infos: Arena<TypeInfo>,
    /// Root defs (packages and top-level members)
    pub roots: Vec<DefId>,
    /// Map from DefId to TypeId (for types that have type info)
    pub def_to_type: Vec<Option<TypeId>>,
}

impl SemanticModel {
    pub fn new() -> Self {
        Self {
            defs: Arena::new(),
            type_infos: Arena::new(),
            roots: Vec::new(),
            def_to_type: Vec::new(),
        }
    }

    pub fn alloc_def(&mut self, def: Def) -> DefId {
        let id = self.defs.alloc(def);
        // Ensure def_to_type is large enough
        while self.def_to_type.len() <= id.raw() as usize {
            self.def_to_type.push(None);
        }
        id
    }

    pub fn add_child(&mut self, parent: DefId, child: DefId) {
        self.defs[parent].children.push(child);
        self.defs[child].parent = Some(parent);
    }

    /// Find a direct child of `parent` with the given name.
    pub fn find_child(&self, parent: DefId, name: SymbolId) -> Option<DefId> {
        self.defs[parent]
            .children
            .iter()
            .find(|&&child| self.defs[child].name == name)
            .copied()
    }

    /// Find a root def with the given name.
    pub fn find_root(&self, name: SymbolId) -> Option<DefId> {
        self.roots
            .iter()
            .find(|&&id| self.defs[id].name == name)
            .copied()
    }
}

impl Default for SemanticModel {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conjugate_in_becomes_out() {
        assert_eq!(
            conjugate_direction(Some(FeatureDirection::In)),
            Some(FeatureDirection::Out)
        );
    }

    #[test]
    fn conjugate_out_becomes_in() {
        assert_eq!(
            conjugate_direction(Some(FeatureDirection::Out)),
            Some(FeatureDirection::In)
        );
    }

    #[test]
    fn conjugate_inout_stays_inout() {
        assert_eq!(
            conjugate_direction(Some(FeatureDirection::InOut)),
            Some(FeatureDirection::InOut)
        );
    }

    #[test]
    fn conjugate_none_stays_none() {
        assert_eq!(conjugate_direction(None), None);
    }
}
