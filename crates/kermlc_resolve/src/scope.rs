use kermlc_hir::{DefId, SemanticModel};
use kermlc_intern::SymbolId;

/// Look up a name in the scope of a given def.
/// Searches: own children, then ancestors' children, then root scope.
pub fn resolve_in_scope(
    model: &SemanticModel,
    scope: DefId,
    name: SymbolId,
) -> Option<DefId> {
    // 1. Search own children
    if let Some(found) = model.find_child(scope, name) {
        return Some(found);
    }

    // 2. Walk up to parent scopes
    let mut current = model.defs[scope].parent;
    while let Some(parent) = current {
        if let Some(found) = model.find_child(parent, name) {
            return Some(found);
        }
        current = model.defs[parent].parent;
    }

    // 3. Search root scope
    if let Some(found) = model.find_root(name) {
        return Some(found);
    }

    None
}

/// Resolve a qualified name (multi-segment) starting from a scope.
/// For `A::B::C`, first resolves `A` in scope, then `B` as child of A, then `C` as child of B.
pub fn resolve_qualified(
    model: &SemanticModel,
    scope: DefId,
    segments: &[SymbolId],
) -> Option<DefId> {
    if segments.is_empty() {
        return None;
    }

    // Resolve the first segment in scope
    let mut current = resolve_in_scope(model, scope, segments[0])?;

    // Resolve subsequent segments as children
    for &seg in &segments[1..] {
        current = model.find_child(current, seg)?;
    }

    Some(current)
}

/// Resolve a qualified name starting from root scope (no enclosing def).
pub fn resolve_qualified_from_root(
    model: &SemanticModel,
    segments: &[SymbolId],
) -> Option<DefId> {
    if segments.is_empty() {
        return None;
    }

    let mut current = model.find_root(segments[0])?;

    for &seg in &segments[1..] {
        current = model.find_child(current, seg)?;
    }

    Some(current)
}

/// Try to resolve imports for a given def's scope.
/// Returns any defs imported by wildcard imports.
pub fn resolve_via_imports(
    model: &SemanticModel,
    scope: DefId,
    name: SymbolId,
) -> Option<DefId> {
    let imports = model.defs[scope].imports.clone();
    for import in &imports {
        if let Some(target) = import.path.resolved_def() {
            if import.is_wildcard {
                // Wildcard import: look for name in the target's children
                if let Some(found) = model.find_child(target, name) {
                    return Some(found);
                }
            } else {
                // Named import: check if the last segment matches
                if let Some(&last) = import.path.segments.last() {
                    if last == name {
                        return Some(target);
                    }
                }
            }
        }
    }
    None
}
