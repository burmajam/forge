use crate::scope::{resolve_qualified, resolve_qualified_from_root, resolve_via_imports};
use kermlc_diagnostics::{Diagnostic, DiagnosticSink, Label};
use kermlc_hir::{DefId, ResolutionState, SemanticModel};
use kermlc_intern::StringInterner;

/// Run one pass of name resolution over the entire model.
/// Returns `true` if any name was resolved (i.e., progress was made).
pub fn resolve_pass(
    model: &mut SemanticModel,
    _interner: &StringInterner,
    _sink: &mut DiagnosticSink,
) -> bool {
    let mut changed = false;

    // Collect all DefIds to iterate over (can't borrow model mutably while iterating)
    let all_defs: Vec<DefId> = model.defs.iter().map(|(id, _)| id).collect();

    for def_id in all_defs {
        // Resolve imports first
        changed |= resolve_imports_for(model, def_id);
        // Resolve specializations
        changed |= resolve_specializations_for(model, def_id);
        // Resolve conjugation
        changed |= resolve_conjugation_for(model, def_id);
        // Resolve type refs
        changed |= resolve_type_ref_for(model, def_id);
        // Resolve feature chains (may be deferred if types aren't known yet)
        changed |= resolve_chains_for(model, def_id);
    }

    changed
}

/// Emit diagnostics for anything still unresolved after fixpoint completes.
pub fn emit_unresolved_errors(
    model: &SemanticModel,
    interner: &StringInterner,
    sink: &mut DiagnosticSink,
) {
    for (_def_id, def) in model.defs.iter() {
        for spec in &def.specializations {
            if spec.resolution == ResolutionState::Unresolved {
                let name_str = segments_to_string(&spec.segments, interner);
                sink.emit(
                    Diagnostic::error(format!("unresolved type `{}`", name_str))
                        .with_label(Label::primary(spec.span, "not found")),
                );
            }
        }

        if let Some(conj) = &def.conjugation {
            if conj.resolution == ResolutionState::Unresolved {
                let name_str = segments_to_string(&conj.segments, interner);
                sink.emit(
                    Diagnostic::error(format!("unresolved conjugation target `{}`", name_str))
                        .with_label(Label::primary(conj.span, "not found")),
                );
            }
        }

        if let Some(type_ref) = &def.type_ref {
            if type_ref.resolution == ResolutionState::Unresolved {
                let name_str = segments_to_string(&type_ref.segments, interner);
                sink.emit(
                    Diagnostic::error(format!("unresolved type `{}`", name_str))
                        .with_label(Label::primary(type_ref.span, "not found")),
                );
            }
        }

        for chain_seg in &def.chain_segments {
            if chain_seg.resolution == ResolutionState::Unresolved {
                let name_str = segments_to_string(&chain_seg.segments, interner);
                sink.emit(
                    Diagnostic::error(format!("unresolved chain segment `{}`", name_str))
                        .with_label(Label::primary(chain_seg.span, "not found")),
                );
            }
        }
    }
}

fn segments_to_string(segments: &[kermlc_intern::SymbolId], interner: &StringInterner) -> String {
    segments
        .iter()
        .map(|s| interner.resolve(*s))
        .collect::<Vec<_>>()
        .join("::")
}

fn resolve_imports_for(model: &mut SemanticModel, def_id: DefId) -> bool {
    let mut changed = false;
    let num_imports = model.defs[def_id].imports.len();

    for i in 0..num_imports {
        if model.defs[def_id].imports[i].path.resolution != ResolutionState::Unresolved {
            continue;
        }

        let segments = model.defs[def_id].imports[i].path.segments.clone();
        if let Some(resolved) = try_resolve_name(model, def_id, &segments) {
            model.defs[def_id].imports[i].path.resolution = ResolutionState::Resolved(resolved);
            changed = true;
        }
    }

    changed
}

fn resolve_specializations_for(model: &mut SemanticModel, def_id: DefId) -> bool {
    let mut changed = false;
    let num_specs = model.defs[def_id].specializations.len();

    for i in 0..num_specs {
        if model.defs[def_id].specializations[i].resolution != ResolutionState::Unresolved {
            continue;
        }

        let segments = model.defs[def_id].specializations[i].segments.clone();
        if let Some(resolved) = try_resolve_name(model, def_id, &segments) {
            model.defs[def_id].specializations[i].resolution =
                ResolutionState::Resolved(resolved);
            changed = true;
        }
    }

    changed
}

fn resolve_conjugation_for(model: &mut SemanticModel, def_id: DefId) -> bool {
    let conj = match &model.defs[def_id].conjugation {
        Some(c) if c.resolution == ResolutionState::Unresolved => c.segments.clone(),
        _ => return false,
    };

    if let Some(resolved) = try_resolve_name(model, def_id, &conj) {
        model.defs[def_id].conjugation.as_mut().unwrap().resolution =
            ResolutionState::Resolved(resolved);
        true
    } else {
        false
    }
}

fn resolve_type_ref_for(model: &mut SemanticModel, def_id: DefId) -> bool {
    let type_ref = match &model.defs[def_id].type_ref {
        Some(tr) if tr.resolution == ResolutionState::Unresolved => tr.segments.clone(),
        _ => return false,
    };

    if let Some(resolved) = try_resolve_name(model, def_id, &type_ref) {
        model.defs[def_id].type_ref.as_mut().unwrap().resolution =
            ResolutionState::Resolved(resolved);
        true
    } else {
        false
    }
}

fn resolve_chains_for(model: &mut SemanticModel, def_id: DefId) -> bool {
    let mut changed = false;
    let num_chains = model.defs[def_id].chain_segments.len();

    for i in 0..num_chains {
        if model.defs[def_id].chain_segments[i].resolution != ResolutionState::Unresolved {
            continue;
        }

        let segments = model.defs[def_id].chain_segments[i].segments.clone();
        if let Some(resolved) = try_resolve_name(model, def_id, &segments) {
            model.defs[def_id].chain_segments[i].resolution =
                ResolutionState::Resolved(resolved);
            changed = true;
        }
    }

    changed
}

/// Try to resolve a multi-segment name from the perspective of `scope`.
fn try_resolve_name(
    model: &SemanticModel,
    scope: DefId,
    segments: &[kermlc_intern::SymbolId],
) -> Option<DefId> {
    if segments.is_empty() {
        return None;
    }

    // Try qualified resolution from this scope
    if let Some(found) = resolve_qualified(model, scope, segments) {
        return Some(found);
    }

    // Try via imports in this scope and parent scopes
    let mut current_scope = Some(scope);
    while let Some(s) = current_scope {
        if let Some(found) = resolve_via_imports(model, s, segments[0]) {
            if segments.len() == 1 {
                return Some(found);
            }
            // Multi-segment: resolve the rest as children
            let mut current = found;
            let mut all_resolved = true;
            for &seg in &segments[1..] {
                if let Some(child) = model.find_child(current, seg) {
                    current = child;
                } else {
                    all_resolved = false;
                    break;
                }
            }
            if all_resolved {
                return Some(current);
            }
        }
        current_scope = model.defs[s].parent;
    }

    // Try from root
    resolve_qualified_from_root(model, segments)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kermlc_diagnostics::{DiagnosticSink, SourceMap};
    use kermlc_hir::lower_ast;
    use kermlc_intern::StringInterner;
    use kermlc_parser::Parser;

    fn parse_and_lower(input: &str) -> (SemanticModel, StringInterner, DiagnosticSink) {
        let mut interner = StringInterner::new();
        let mut source_map = SourceMap::new();
        let mut sink = DiagnosticSink::new();
        let file_id = source_map.add_file("test.kerml".into(), input.into());
        let parse = Parser::parse(input, file_id, &mut interner, &mut sink);
        let model = lower_ast(&parse, &interner, &mut sink);
        (model, interner, sink)
    }

    #[test]
    fn resolve_local_type() {
        let (mut model, interner, mut sink) =
            parse_and_lower("package P { type A {} type B :> A {} }");
        assert!(!sink.has_errors());

        let changed = resolve_pass(&mut model, &interner, &mut sink);
        assert!(changed);

        // Find B's specialization
        let pkg = model.roots[0];
        let b_id = model.defs[pkg].children[1];
        assert!(model.defs[b_id].specializations[0].is_resolved());
    }

    #[test]
    fn resolve_qualified_name() {
        let (mut model, interner, mut sink) = parse_and_lower(
            "package A { type X {} } package B { type Y :> A::X {} }",
        );
        assert!(!sink.has_errors());

        let changed = resolve_pass(&mut model, &interner, &mut sink);
        assert!(changed);

        // Find Y in package B
        let b_pkg = model.roots[1];
        let y_id = model.defs[b_pkg].children[0];
        assert!(model.defs[y_id].specializations[0].is_resolved());
    }

    #[test]
    fn resolve_import() {
        let (mut model, interner, mut sink) = parse_and_lower(
            "package A { type X {} } package B { import A::*; type Y :> X {} }",
        );
        assert!(!sink.has_errors());

        // First pass: resolve imports, then a second pass resolves the type ref
        resolve_pass(&mut model, &interner, &mut sink);
        resolve_pass(&mut model, &interner, &mut sink);

        let b_pkg = model.roots[1];
        let y_id = model.defs[b_pkg].children[0];
        assert!(
            model.defs[y_id].specializations[0].is_resolved(),
            "Y's specialization of X should resolve via import"
        );
    }

    #[test]
    fn unresolved_name_produces_error() {
        let (mut model, interner, mut sink) =
            parse_and_lower("package P { type A :> NonExistent {} }");

        // Run resolution
        resolve_pass(&mut model, &interner, &mut sink);

        // Should still be unresolved
        let pkg = model.roots[0];
        let a_id = model.defs[pkg].children[0];
        assert!(!model.defs[a_id].specializations[0].is_resolved());

        // Emit errors for unresolved names
        emit_unresolved_errors(&model, &interner, &mut sink);
        assert!(sink.has_errors());
    }
}
