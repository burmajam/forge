use kermlc_diagnostics::DiagnosticSink;
use kermlc_hir::{
    conjugate_direction, DefId, DefKind, InheritanceKind, InheritedFeature, SemanticModel,
};
use kermlc_intern::StringInterner;

/// Run one pass of type checking over the model.
/// Processes resolved specializations to compute inherited features.
/// Returns `true` if any type information changed (for the fixpoint loop).
pub fn typecheck_pass(
    model: &mut SemanticModel,
    _interner: &StringInterner,
    _sink: &mut DiagnosticSink,
) -> bool {
    let mut changed = false;

    let all_defs: Vec<DefId> = model.defs.iter().map(|(id, _)| id).collect();

    for def_id in all_defs {
        if model.defs[def_id].type_checked {
            continue;
        }

        match model.defs[def_id].kind {
            DefKind::Type => {
                changed |= check_type(model, def_id);
            }
            DefKind::Feature => {
                changed |= check_feature(model, def_id);
            }
            DefKind::Conjugation => {
                changed |= check_conjugation_decl(model, def_id);
            }
            DefKind::Package => {}
        }
    }

    changed
}

/// Check a type definition: verify specializations are resolved and compute inherited features.
fn check_type(model: &mut SemanticModel, def_id: DefId) -> bool {
    let mut changed = false;

    // Check if all specializations are resolved
    let all_specs_resolved = model.defs[def_id]
        .specializations
        .iter()
        .all(|s| s.is_resolved());

    if !all_specs_resolved {
        return false; // Can't type-check yet, defer
    }

    // Check conjugation if present
    if let Some(conj) = &model.defs[def_id].conjugation {
        if !conj.is_resolved() {
            return false; // Can't type-check yet
        }
    }

    // Collect inherited features from supertypes
    let supertype_ids: Vec<DefId> = model.defs[def_id]
        .specializations
        .iter()
        .filter_map(|s| s.resolved_def())
        .collect();

    let mut inherited: Vec<InheritedFeature> = Vec::new();
    for &super_id in &supertype_ids {
        // Collect own features of the supertype
        let super_features: Vec<DefId> = model.defs[super_id]
            .children
            .iter()
            .filter(|&&c| model.defs[c].kind == DefKind::Feature)
            .copied()
            .collect();
        for f in super_features {
            inherited.push(InheritedFeature {
                def_id: f,
                kind: InheritanceKind::Specialization,
                direction_override: None,
            });
        }

        // Also collect inherited features of the supertype
        let super_inherited = model.defs[super_id].inherited_features.clone();
        inherited.extend(super_inherited);
    }

    // Deduplicate by def_id
    inherited.sort_by_key(|f| f.def_id.raw());
    inherited.dedup_by_key(|f| f.def_id);

    if model.defs[def_id].inherited_features != inherited {
        model.defs[def_id].inherited_features = inherited;
        changed = true;
    }

    // Handle conjugation: inherit features with flipped directions
    if let Some(conj) = &model.defs[def_id].conjugation {
        if let Some(conj_target) = conj.resolved_def() {
            // Direct features of conjugated type
            let conj_features: Vec<DefId> = model.defs[conj_target]
                .children
                .iter()
                .filter(|&&c| model.defs[c].kind == DefKind::Feature)
                .copied()
                .collect();

            let mut all_conj: Vec<InheritedFeature> = conj_features
                .into_iter()
                .map(|f| InheritedFeature {
                    def_id: f,
                    kind: InheritanceKind::Conjugation,
                    direction_override: conjugate_direction(model.defs[f].direction),
                })
                .collect();

            // Inherited features of conjugated type
            let conj_inherited: Vec<InheritedFeature> = model.defs[conj_target]
                .inherited_features
                .iter()
                .map(|inh| {
                    let effective = inh.direction_override.or(model.defs[inh.def_id].direction);
                    InheritedFeature {
                        def_id: inh.def_id,
                        kind: InheritanceKind::Conjugation,
                        direction_override: conjugate_direction(effective),
                    }
                })
                .collect();
            all_conj.extend(conj_inherited);

            all_conj.sort_by_key(|f| f.def_id.raw());
            all_conj.dedup_by_key(|f| f.def_id);

            // Populate conjugate_of in TypeInfo
            let type_id = model.def_to_type[def_id.raw() as usize];
            if let Some(tid) = type_id {
                model.type_infos[tid].conjugate_of = Some(conj_target);
            }

            // Add conjugated features (avoiding duplicates)
            for f in all_conj {
                let dominated = model.defs[def_id]
                    .inherited_features
                    .iter()
                    .any(|existing| existing.def_id == f.def_id);
                if !dominated {
                    model.defs[def_id].inherited_features.push(f);
                    changed = true;
                }
            }
        }
    }

    model.defs[def_id].type_checked = true;
    changed
}

/// Check a named conjugation declaration: apply conjugation
/// effect to the conjugated type.
fn check_conjugation_decl(model: &mut SemanticModel, def_id: DefId) -> bool {
    let (conj_ref, orig_ref) = match &model.defs[def_id].conjugation_decl {
        Some((c, o)) => (c.clone(), o.clone()),
        None => return false,
    };

    if !conj_ref.is_resolved() || !orig_ref.is_resolved() {
        return false;
    }

    let conjugated_id = conj_ref.resolved_def().unwrap();
    let original_id = orig_ref.resolved_def().unwrap();

    // Only apply if the conjugated type doesn't already have
    // a conjugation set (from inline ~ or a prior declaration)
    if model.defs[conjugated_id].conjugation.is_some() {
        model.defs[def_id].type_checked = true;
        return false;
    }

    // Set the conjugation on the conjugated type so check_type
    // picks it up and applies direction flipping
    model.defs[conjugated_id].conjugation = Some(kermlc_hir::NameRef {
        segments: orig_ref.segments,
        span: orig_ref.span,
        resolution: kermlc_hir::ResolutionState::Resolved(original_id),
    });

    // Reset type_checked on the conjugated type so it gets
    // re-processed with the new conjugation
    model.defs[conjugated_id].type_checked = false;

    model.defs[def_id].type_checked = true;
    true
}

/// Check a feature definition: verify type ref is resolved.
fn check_feature(model: &mut SemanticModel, def_id: DefId) -> bool {
    // Check type ref
    if let Some(type_ref) = &model.defs[def_id].type_ref {
        if !type_ref.is_resolved() {
            return false; // Can't type-check yet
        }
    }

    // Check chain segments
    let all_chains_resolved = model.defs[def_id]
        .chain_segments
        .iter()
        .all(|s| s.is_resolved());

    if !all_chains_resolved {
        return false;
    }

    model.defs[def_id].type_checked = true;
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use kermlc_diagnostics::{DiagnosticSink, SourceMap};
    use kermlc_hir::{lower_ast, FeatureDirection};
    use kermlc_intern::StringInterner;
    use kermlc_parser::Parser;
    use kermlc_resolve::resolve_pass;

    fn compile_to_model(input: &str) -> (SemanticModel, StringInterner, DiagnosticSink) {
        let mut interner = StringInterner::new();
        let mut source_map = SourceMap::new();
        let mut sink = DiagnosticSink::new();
        let file_id = source_map.add_file("test.kerml".into(), input.into());
        let parse = Parser::parse(input, file_id, &mut interner, &mut sink);
        let mut model = lower_ast(&parse, &interner, &mut sink);

        // Run fixpoint
        for _ in 0..10 {
            let r = resolve_pass(&mut model, &interner, &mut sink);
            let t = typecheck_pass(&mut model, &interner, &mut sink);
            if !r && !t {
                break;
            }
        }

        (model, interner, sink)
    }

    #[test]
    fn specialization_adds_inherited_features() {
        let (model, interner, sink) =
            compile_to_model("package P { type A { feature x : A; } type B :> A {} }");
        assert!(!sink.has_errors());

        // Find B
        let pkg = model.roots[0];
        let b_id = model.defs[pkg].children[1];
        assert_eq!(interner.resolve(model.defs[b_id].name), "B");
        assert!(model.defs[b_id].type_checked);

        // B should inherit feature x from A
        assert!(
            !model.defs[b_id].inherited_features.is_empty(),
            "B should inherit features from A"
        );
    }

    #[test]
    fn conjugation_inherits_features() {
        let (model, interner, sink) =
            compile_to_model("package P { type T { feature f : T; } type U ~ T {} }");
        assert!(!sink.has_errors());

        let pkg = model.roots[0];
        let u_id = model.defs[pkg].children[1];
        assert_eq!(interner.resolve(model.defs[u_id].name), "U");
        assert!(model.defs[u_id].type_checked);
        assert!(
            !model.defs[u_id].inherited_features.is_empty(),
            "U should inherit features from conjugated T"
        );
    }

    #[test]
    fn conjugation_flips_directions() {
        let (model, interner, _sink) = compile_to_model(
            r#"package P {
            type A {
                in feature f : A;
                out feature g : A;
                inout feature h : A;
                feature x : A;
            }
            type B ~ A {}
        }"#,
        );

        let pkg = model.roots[0];
        let b_id = model.defs[pkg].children[1];
        assert_eq!(interner.resolve(model.defs[b_id].name), "B");

        let inherited = &model.defs[b_id].inherited_features;
        assert_eq!(inherited.len(), 4);

        for inh in inherited {
            let name = interner.resolve(model.defs[inh.def_id].name);
            assert_eq!(inh.kind, InheritanceKind::Conjugation);
            match name {
                "f" => assert_eq!(inh.direction_override, Some(FeatureDirection::Out)),
                "g" => assert_eq!(inh.direction_override, Some(FeatureDirection::In)),
                "h" => assert_eq!(inh.direction_override, Some(FeatureDirection::InOut)),
                "x" => {
                    assert_eq!(inh.direction_override, None)
                }
                other => panic!("unexpected feature: {other}"),
            }
        }
    }

    #[test]
    fn feature_type_ref_resolved() {
        let (model, interner, sink) =
            compile_to_model("package P { type A {} type B { feature x : A; } }");
        assert!(!sink.has_errors());

        let pkg = model.roots[0];
        let b_id = model.defs[pkg].children[1];
        let x_id = model.defs[b_id].children[0];
        assert_eq!(interner.resolve(model.defs[x_id].name), "x");
        assert!(model.defs[x_id].type_ref.as_ref().unwrap().is_resolved());
        assert!(model.defs[x_id].type_checked);
    }
}
