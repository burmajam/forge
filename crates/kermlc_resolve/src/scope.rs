use kermlc_hir::{DefId, SemanticModel};
use kermlc_intern::SymbolId;

/// Look up a name in the scope of a given def.
/// Searches: own children, then ancestors' children, then root scope.
pub fn resolve_in_scope(model: &SemanticModel, scope: DefId, name: SymbolId) -> Option<DefId> {
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
pub fn resolve_qualified_from_root(model: &SemanticModel, segments: &[SymbolId]) -> Option<DefId> {
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
pub fn resolve_via_imports(model: &SemanticModel, scope: DefId, name: SymbolId) -> Option<DefId> {
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

/// Find a member of a type by name.
/// Searches direct children, then inherited features.
/// No parent walking, no imports — strict member lookup only.
/// Used for type-directed chain resolution (A3).
pub fn find_member(
    model: &SemanticModel,
    type_def_id: DefId,
    name: SymbolId,
) -> Option<DefId> {
    // 1. Direct children
    if let Some(found) = model.find_child(type_def_id, name) {
        return Some(found);
    }

    // 2. Inherited features (populated by typeck)
    for inherited in &model.defs[type_def_id].inherited_features {
        if model.defs[inherited.def_id].name == name {
            return Some(inherited.def_id);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use kermlc_diagnostics::{DiagnosticSink, SourceMap};
    use kermlc_hir::lower_ast;
    use kermlc_intern::StringInterner;
    use kermlc_parser::Parser;

    fn parse_and_lower(
        input: &str,
    ) -> (SemanticModel, StringInterner, DiagnosticSink) {
        let mut interner = StringInterner::new();
        let mut source_map = SourceMap::new();
        let mut sink = DiagnosticSink::new();
        let file_id =
            source_map.add_file("test.kerml".into(), input.into());
        let parse =
            Parser::parse(input, file_id, &mut interner, &mut sink);
        let model = lower_ast(&parse, &mut interner, &mut sink);
        (model, interner, sink)
    }

    #[test]
    fn find_member_direct_child() {
        let (mut model, interner, mut sink) = parse_and_lower(
            "package P { type T { feature f : T; } }",
        );
        crate::resolve_pass(&mut model, &interner, &mut sink);

        let pkg = model.roots[0];
        let t_id = model.defs[pkg].children[0];
        let f_id = model.defs[t_id].children[0];
        let f_name = model.defs[f_id].name;

        let found = find_member(&model, t_id, f_name);
        assert_eq!(found, Some(f_id));
    }

    #[test]
    fn find_member_not_found() {
        let (model, mut interner, _sink) = parse_and_lower(
            "package P { type T { feature f : T; } }",
        );

        let pkg = model.roots[0];
        let t_id = model.defs[pkg].children[0];
        let bad_name = interner.intern("nonexistent");

        let found = find_member(&model, t_id, bad_name);
        assert_eq!(found, None);
    }

    #[test]
    fn find_member_inherited_feature() {
        let (mut model, interner, mut sink) = parse_and_lower(
            "package P { type A { feature x : A; } type B :> A {} }",
        );
        for _ in 0..10 {
            let r = crate::resolve_pass(
                &mut model, &interner, &mut sink,
            );
            let t = kermlc_typeck::typecheck_pass(
                &mut model, &interner, &mut sink,
            );
            if !r && !t {
                break;
            }
        }

        let pkg = model.roots[0];
        let a_id = model.defs[pkg].children[0];
        let x_id = model.defs[a_id].children[0];
        let x_name = model.defs[x_id].name;
        let b_id = model.defs[pkg].children[1];

        let found = find_member(&model, b_id, x_name);
        assert_eq!(found, Some(x_id));
    }

    #[test]
    fn find_member_no_parent_walking() {
        let (model, mut interner, _sink) = parse_and_lower(
            "package P { type T {} feature outside : T; }",
        );

        let pkg = model.roots[0];
        let t_id = model.defs[pkg].children[0];
        let outside_name = interner.intern("outside");

        let found = find_member(&model, t_id, outside_name);
        assert_eq!(found, None);
    }
}
