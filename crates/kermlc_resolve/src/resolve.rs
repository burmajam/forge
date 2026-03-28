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
            emit_unresolved(spec, "type", interner, sink);
        }
        if let Some(conj) = &def.conjugation {
            emit_unresolved(conj, "conjugation target", interner, sink);
        }
        if let Some(type_ref) = &def.type_ref {
            emit_unresolved(type_ref, "type", interner, sink);
        }
        if let Some((ref conj, ref orig)) = def.conjugation_decl {
            emit_unresolved(conj, "conjugated type", interner, sink);
            emit_unresolved(orig, "original type", interner, sink);
        }
        for chain_seg in &def.chain_segments {
            emit_unresolved(chain_seg, "chain segment", interner, sink);
            if chain_seg.resolution == ResolutionState::Error {
                let name_str =
                    segments_to_string(&chain_seg.segments, interner);
                sink.emit(
                    Diagnostic::error(format!(
                        "unresolved chain segment `{name_str}`"
                    ))
                    .with_label(Label::primary(
                        chain_seg.span,
                        "member not found in type",
                    )),
                );
            }
        }
        if let Some(ref mult) = def.multiplicity {
            for bound in [&mult.lower, &mult.upper] {
                if let kermlc_hir::MultBound::Ref(ref r) = bound {
                    emit_unresolved(
                        r,
                        "multiplicity bound",
                        interner,
                        sink,
                    );
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

fn emit_unresolved(
    nr: &kermlc_hir::NameRef,
    kind: &str,
    interner: &StringInterner,
    sink: &mut DiagnosticSink,
) {
    if nr.resolution == ResolutionState::Unresolved {
        let name_str = segments_to_string(&nr.segments, interner);
        sink.emit(
            Diagnostic::error(format!("unresolved {kind} `{name_str}`"))
                .with_label(Label::primary(nr.span, "not found")),
        );
    }
}

fn segments_to_string(segments: &[kermlc_intern::SymbolId], interner: &StringInterner) -> String {
    segments
        .iter()
        .map(|s| interner.resolve(*s))
        .collect::<Vec<_>>()
        .join("::")
}

/// Resolve all unresolved `NameRef`s in a `Vec` field of `Def`.
fn resolve_name_ref_vec(
    model: &mut SemanticModel,
    def_id: DefId,
    get: fn(&kermlc_hir::Def) -> &[kermlc_hir::NameRef],
    get_mut: fn(&mut kermlc_hir::Def) -> &mut [kermlc_hir::NameRef],
) -> bool {
    let count = get(&model.defs[def_id]).len();
    let mut changed = false;
    for i in 0..count {
        if get(&model.defs[def_id])[i].resolution != ResolutionState::Unresolved {
            continue;
        }
        let segments = get(&model.defs[def_id])[i].segments.clone();
        if let Some(resolved) = try_resolve_name(model, def_id, &segments) {
            get_mut(&mut model.defs[def_id])[i].resolution =
                ResolutionState::Resolved(resolved);
            changed = true;
        }
    }
    changed
}

/// Resolve an unresolved `Option<NameRef>` field of `Def`.
fn resolve_optional_ref(
    model: &mut SemanticModel,
    def_id: DefId,
    get: fn(&kermlc_hir::Def) -> Option<&kermlc_hir::NameRef>,
    set: fn(&mut kermlc_hir::Def, ResolutionState),
) -> bool {
    let segments = match get(&model.defs[def_id]) {
        Some(nr) if nr.resolution == ResolutionState::Unresolved => {
            nr.segments.clone()
        }
        _ => return false,
    };
    let Some(resolved) = try_resolve_name(model, def_id, &segments) else {
        return false;
    };
    set(
        &mut model.defs[def_id],
        ResolutionState::Resolved(resolved),
    );
    true
}

fn resolve_imports_for(model: &mut SemanticModel, def_id: DefId) -> bool {
    let count = model.defs[def_id].imports.len();
    let mut changed = false;
    for i in 0..count {
        if model.defs[def_id].imports[i].path.resolution
            != ResolutionState::Unresolved
        {
            continue;
        }
        let segments = model.defs[def_id].imports[i].path.segments.clone();
        if let Some(resolved) = try_resolve_name(model, def_id, &segments) {
            model.defs[def_id].imports[i].path.resolution =
                ResolutionState::Resolved(resolved);
            changed = true;
        }
    }
    changed
}

fn resolve_specializations_for(
    model: &mut SemanticModel,
    def_id: DefId,
) -> bool {
    resolve_name_ref_vec(
        model,
        def_id,
        |d| &d.specializations,
        |d| &mut d.specializations,
    )
}

fn resolve_conjugation_for(model: &mut SemanticModel, def_id: DefId) -> bool {
    resolve_optional_ref(
        model,
        def_id,
        |d| d.conjugation.as_ref(),
        |d, res| d.conjugation.as_mut().unwrap().resolution = res,
    )
}

fn resolve_conjugation_decl_for(
    model: &mut SemanticModel,
    def_id: DefId,
) -> bool {
    if model.defs[def_id].kind != kermlc_hir::DefKind::Conjugation {
        return false;
    }
    let mut changed = false;
    changed |= resolve_optional_ref(
        model,
        def_id,
        |d| d.conjugation_decl.as_ref().map(|(c, _)| c),
        |d, res| d.conjugation_decl.as_mut().unwrap().0.resolution = res,
    );
    changed |= resolve_optional_ref(
        model,
        def_id,
        |d| d.conjugation_decl.as_ref().map(|(_, o)| o),
        |d, res| d.conjugation_decl.as_mut().unwrap().1.resolution = res,
    );
    changed
}

fn resolve_type_ref_for(model: &mut SemanticModel, def_id: DefId) -> bool {
    resolve_optional_ref(
        model,
        def_id,
        |d| d.type_ref.as_ref(),
        |d, res| d.type_ref.as_mut().unwrap().resolution = res,
    )
}

fn resolve_chains_for(model: &mut SemanticModel, def_id: DefId) -> bool {
    let count = model.defs[def_id].chain_segments.len();
    if count == 0 {
        return false;
    }

    let mut changed = false;

    for i in 0..count {
        if model.defs[def_id].chain_segments[i].resolution
            != ResolutionState::Unresolved
        {
            continue;
        }

        if i == 0 {
            // First segment: scope-based resolution (as before)
            let segments =
                model.defs[def_id].chain_segments[0].segments.clone();
            if let Some(resolved) =
                try_resolve_name(model, def_id, &segments)
            {
                model.defs[def_id].chain_segments[0].resolution =
                    ResolutionState::Resolved(resolved);
                changed = true;
            }
        } else {
            // Subsequent segments: type-directed via find_member
            let prev_resolution =
                model.defs[def_id].chain_segments[i - 1].resolution.clone();
            let prev_def = match prev_resolution {
                ResolutionState::Resolved(id) => id,
                _ => break,
            };

            // Get the type of the previous segment
            let prev_type_ref = match &model.defs[prev_def].type_ref {
                Some(tr) => tr.resolution.clone(),
                None => break,
            };
            let type_def = match prev_type_ref {
                ResolutionState::Resolved(id) => id,
                _ => break,
            };

            // Look up this segment as a member of that type
            let seg_name =
                model.defs[def_id].chain_segments[i].segments[0];
            if let Some(found) =
                crate::scope::find_member(model, type_def, seg_name)
            {
                model.defs[def_id].chain_segments[i].resolution =
                    ResolutionState::Resolved(found);
                changed = true;
            } else if model.defs[type_def].type_checked {
                model.defs[def_id].chain_segments[i].resolution =
                    ResolutionState::Error;
                changed = true;
                break;
            } else {
                break;
            }
        }
    }

    // If all segments resolved, set chain_result to the last one
    let all_resolved = (0..count).all(|i| {
        matches!(
            model.defs[def_id].chain_segments[i].resolution,
            ResolutionState::Resolved(_)
        )
    });
    if all_resolved && model.defs[def_id].chain_result.is_none() {
        if let ResolutionState::Resolved(last) =
            model.defs[def_id].chain_segments[count - 1].resolution
        {
            model.defs[def_id].chain_result = Some(last);
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

    #[test]
    fn resolve_chain_type_directed() {
        let src = "\
            package P {\
                type Engine { feature cylinders : Engine; }\
                type Vehicle { feature engine : Engine; }\
                type Fleet {\
                    feature vehicles : Vehicle;\
                    feature v_eng chains vehicles.engine;\
                }\
            }";
        let (mut model, mut interner, mut sink) = parse_and_lower(src);
        let stdlib =
            kermlc_hir::load_stdlib(&mut model, &mut interner);
        kermlc_hir::add_implicit_specializations(&mut model, &stdlib);

        for _ in 0..100 {
            let r =
                resolve_pass(&mut model, &interner, &mut sink);
            let t = kermlc_typeck::typecheck_pass(
                &mut model, &interner, &mut sink,
            );
            if !r && !t {
                break;
            }
        }
        emit_unresolved_errors(&model, &interner, &mut sink);

        assert!(
            !sink.has_errors(),
            "chain should resolve: {:?}",
            sink.diagnostics()
        );

        let pkg = model.roots[0];
        let fleet = model.defs[pkg].children[2]; // Fleet
        let v_eng = model.defs[fleet].children[1]; // v_eng
        assert!(
            model.defs[v_eng].chain_result.is_some(),
            "chain_result should be set after resolution"
        );
    }

    #[test]
    fn resolve_chain_three_steps() {
        let src = "\
            package P {\
                type C {}\
                type B { feature c : C; }\
                type A { feature b : B; }\
                type Root {\
                    feature a : A;\
                    feature abc chains a.b.c;\
                }\
            }";
        let (mut model, mut interner, mut sink) = parse_and_lower(src);
        let stdlib =
            kermlc_hir::load_stdlib(&mut model, &mut interner);
        kermlc_hir::add_implicit_specializations(&mut model, &stdlib);

        for _ in 0..100 {
            let r =
                resolve_pass(&mut model, &interner, &mut sink);
            let t = kermlc_typeck::typecheck_pass(
                &mut model, &interner, &mut sink,
            );
            if !r && !t {
                break;
            }
        }
        emit_unresolved_errors(&model, &interner, &mut sink);

        assert!(
            !sink.has_errors(),
            "3-step chain should resolve: {:?}",
            sink.diagnostics()
        );

        let pkg = model.roots[0];
        let root = model.defs[pkg].children[3]; // Root
        let abc = model.defs[root].children[1]; // abc

        assert!(
            model.defs[abc]
                .chain_segments
                .iter()
                .all(|s| s.is_resolved()),
            "all chain segments should be resolved"
        );
        assert!(
            model.defs[abc].chain_result.is_some(),
            "chain_result should be set"
        );
    }

    #[test]
    fn resolve_chain_unresolved_member() {
        let src = "\
            package P {\
                type A { feature x : A; }\
                type Root {\
                    feature a : A;\
                    feature bad chains a.nonexistent;\
                }\
            }";
        let (mut model, mut interner, mut sink) = parse_and_lower(src);
        let stdlib =
            kermlc_hir::load_stdlib(&mut model, &mut interner);
        kermlc_hir::add_implicit_specializations(&mut model, &stdlib);

        for _ in 0..100 {
            let r =
                resolve_pass(&mut model, &interner, &mut sink);
            let t = kermlc_typeck::typecheck_pass(
                &mut model, &interner, &mut sink,
            );
            if !r && !t {
                break;
            }
        }
        emit_unresolved_errors(&model, &interner, &mut sink);

        assert!(
            sink.has_errors(),
            "unresolved chain member should produce error"
        );
    }
}
