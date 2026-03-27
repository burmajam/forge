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
        // Resolve conjugation declarations
        changed |= resolve_conjugation_decl_for(model, def_id);
        // Resolve type refs
        changed |= resolve_type_ref_for(model, def_id);
        // Resolve feature chains (may be deferred if types aren't known yet)
        changed |= resolve_chains_for(model, def_id);
        // Resolve multiplicity refs
        changed |= resolve_multiplicity_refs_for(model, def_id);
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

        if let Some((ref conj, ref orig)) = def.conjugation_decl {
            if conj.resolution == ResolutionState::Unresolved {
                let name_str = segments_to_string(&conj.segments, interner);
                sink.emit(
                    Diagnostic::error(format!("unresolved conjugated type `{name_str}`"))
                        .with_label(Label::primary(conj.span, "not found")),
                );
            }
            if orig.resolution == ResolutionState::Unresolved {
                let name_str = segments_to_string(&orig.segments, interner);
                sink.emit(
                    Diagnostic::error(format!("unresolved original type `{name_str}`"))
                        .with_label(Label::primary(orig.span, "not found")),
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
    }
}

/// Detect cycles in the specialization graph among resolved types.
/// Returns `true` if any cycle was found, emitting diagnostics.
///
/// Uses iterative DFS with three colors:
/// - white (unvisited), gray (in current path), black (fully explored).
pub fn detect_specialization_cycles(
    model: &SemanticModel,
    interner: &StringInterner,
    sink: &mut DiagnosticSink,
) -> bool {
    let all_defs: Vec<DefId> = model
        .defs
        .iter()
        .filter(|(_, d)| d.kind == kermlc_hir::DefKind::Type)
        .map(|(id, _)| id)
        .collect();

    #[derive(Clone, Copy, PartialEq)]
    enum Color {
        White,
        Gray,
        Black,
    }

    let capacity = model.defs.len();
    let mut color = vec![Color::White; capacity];
    let mut found_cycle = false;

    for start in &all_defs {
        if color[start.raw() as usize] != Color::White {
            continue;
        }

        // Iterative DFS using an explicit stack
        let mut stack: Vec<(DefId, bool)> = vec![(*start, false)];
        while let Some((node, returning)) = stack.pop() {
            let idx = node.raw() as usize;

            if returning {
                color[idx] = Color::Black;
                continue;
            }

            if color[idx] == Color::Black {
                continue;
            }

            if color[idx] == Color::Gray {
                // Back edge — cycle detected
                let name = interner.resolve(model.defs[node].name);
                sink.emit(
                    Diagnostic::error(format!(
                        "circular specialization: `{}` is part \
                         of a specialization cycle",
                        name
                    ))
                    .with_label(Label::primary(model.defs[node].span, "cycle detected here")),
                );
                found_cycle = true;
                continue;
            }

            color[idx] = Color::Gray;
            // Push return marker
            stack.push((node, true));

            // Push resolved specialization targets
            for spec in &model.defs[node].specializations {
                if let ResolutionState::Resolved(target) = spec.resolution {
                    stack.push((target, false));
                }
            }
        }
    }

    found_cycle
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
            model.defs[def_id].specializations[i].resolution = ResolutionState::Resolved(resolved);
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

fn resolve_conjugation_decl_for(model: &mut SemanticModel, def_id: DefId) -> bool {
    if model.defs[def_id].kind != kermlc_hir::DefKind::Conjugation {
        return false;
    }
    let mut changed = false;

    if let Some((ref conj, _)) = model.defs[def_id].conjugation_decl {
        if conj.resolution == ResolutionState::Unresolved {
            let segs = conj.segments.clone();
            if let Some(resolved) = try_resolve_name(model, def_id, &segs) {
                model.defs[def_id]
                    .conjugation_decl
                    .as_mut()
                    .unwrap()
                    .0
                    .resolution = ResolutionState::Resolved(resolved);
                changed = true;
            }
        }
    }

    if let Some((_, ref orig)) = model.defs[def_id].conjugation_decl {
        if orig.resolution == ResolutionState::Unresolved {
            let segs = orig.segments.clone();
            if let Some(resolved) = try_resolve_name(model, def_id, &segs) {
                model.defs[def_id]
                    .conjugation_decl
                    .as_mut()
                    .unwrap()
                    .1
                    .resolution = ResolutionState::Resolved(resolved);
                changed = true;
            }
        }
    }

    changed
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
            model.defs[def_id].chain_segments[i].resolution = ResolutionState::Resolved(resolved);
            changed = true;
        }
    }

    changed
}

fn resolve_multiplicity_refs_for(model: &mut SemanticModel, def_id: DefId) -> bool {
    if model.defs[def_id].multiplicity.is_none() {
        return false;
    }

    let mut changed = false;
    changed |= resolve_mult_bound(model, def_id, |m| &m.lower, |m| &mut m.lower);
    changed |= resolve_mult_bound(model, def_id, |m| &m.upper, |m| &mut m.upper);
    changed
}

fn resolve_mult_bound(
    model: &mut SemanticModel,
    def_id: DefId,
    get: fn(&kermlc_hir::HirMultiplicity) -> &kermlc_hir::MultBound,
    get_mut: fn(&mut kermlc_hir::HirMultiplicity) -> &mut kermlc_hir::MultBound,
) -> bool {
    let mult = model.defs[def_id]
        .multiplicity
        .as_ref()
        .expect("caller checked is_some");
    let segments = match get(mult) {
        kermlc_hir::MultBound::Ref(nr) if nr.resolution == ResolutionState::Unresolved => {
            nr.segments.clone()
        }
        _ => return false,
    };

    let Some(resolved) = try_resolve_name(model, def_id, &segments) else {
        return false;
    };

    let bound = get_mut(
        model.defs[def_id]
            .multiplicity
            .as_mut()
            .expect("caller checked is_some"),
    );
    if let Some(nr) = bound.as_name_ref_mut() {
        nr.resolution = ResolutionState::Resolved(resolved);
        return true;
    }
    false
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
        let model = lower_ast(&parse, &mut interner, &mut sink);
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
        let (mut model, interner, mut sink) =
            parse_and_lower("package A { type X {} } package B { type Y :> A::X {} }");
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
        let (mut model, interner, mut sink) =
            parse_and_lower("package A { type X {} } package B { import A::*; type Y :> X {} }");
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

    #[test]
    fn direct_cycle_detected_as_error() {
        // A :> B, B :> A — direct cycle
        let (mut model, interner, mut sink) =
            parse_and_lower("package P { type A :> B {} type B :> A {} }");

        // Resolve names first
        resolve_pass(&mut model, &interner, &mut sink);

        // Detect cycles
        let found = detect_specialization_cycles(&model, &interner, &mut sink);
        assert!(found, "should detect cycle between A and B");
        assert!(sink.has_errors());
    }

    #[test]
    fn self_cycle_detected_as_error() {
        // A :> A — self-referential
        let (mut model, interner, mut sink) = parse_and_lower("package P { type A :> A {} }");

        resolve_pass(&mut model, &interner, &mut sink);

        let found = detect_specialization_cycles(&model, &interner, &mut sink);
        assert!(found, "should detect self-cycle on A");
        assert!(sink.has_errors());
    }

    #[test]
    fn transitive_cycle_detected() {
        // A :> B, B :> C, C :> A — 3-node cycle
        let (mut model, interner, mut sink) =
            parse_and_lower("package P { type A :> B {} type B :> C {} type C :> A {} }");

        resolve_pass(&mut model, &interner, &mut sink);

        let found = detect_specialization_cycles(&model, &interner, &mut sink);
        assert!(found, "should detect transitive cycle A -> B -> C -> A");
        assert!(sink.has_errors());
    }

    #[test]
    fn no_cycle_in_valid_chain() {
        // A :> B, B :> C — no cycle
        let (mut model, interner, mut sink) =
            parse_and_lower("package P { type C {} type B :> C {} type A :> B {} }");

        resolve_pass(&mut model, &interner, &mut sink);

        let found = detect_specialization_cycles(&model, &interner, &mut sink);
        assert!(!found, "should not detect cycle in a valid chain");
        assert!(!sink.has_errors());
    }

    #[test]
    fn resolve_multiplicity_feature_ref() {
        let (mut model, interner, mut sink) =
            parse_and_lower("package P { type T { feature n : T; feature x : T [1..n]; } }");
        assert!(!sink.has_errors(), "parse errors: {:?}", sink.diagnostics());

        resolve_pass(&mut model, &interner, &mut sink);

        let pkg = model.roots[0];
        let ty = model.defs[pkg].children[0];
        let x_id = model.defs[ty].children[1];
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
        let (mut model, interner, mut sink) =
            parse_and_lower("package P { type T { feature x : T [1..noSuchFeature]; } }");

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
}
