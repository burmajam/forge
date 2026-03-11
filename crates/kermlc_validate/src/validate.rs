use kermlc_diagnostics::{Diagnostic, DiagnosticSink, Label};
use kermlc_hir::{Bound, DefId, DefKind, SemanticModel};
use kermlc_intern::StringInterner;

/// Run semantic validation on a fully resolved model.
/// Returns `true` if any validation errors were found.
pub fn validate(
    model: &SemanticModel,
    interner: &StringInterner,
    sink: &mut DiagnosticSink,
) -> bool {
    let initial_errors = sink.has_errors();

    let all_defs: Vec<DefId> = model.defs.iter().map(|(id, _)| id).collect();

    for def_id in all_defs {
        match model.defs[def_id].kind {
            DefKind::Type => {
                validate_type(model, interner, def_id, sink);
            }
            DefKind::Feature => {
                validate_feature(model, interner, def_id, sink);
            }
            DefKind::Conjugation => {
                validate_conjugation_decl(model, interner, def_id, sink);
            }
            DefKind::Package => {}
        }
    }

    // Return true if new errors were added
    sink.has_errors() && !initial_errors
}

/// Validate a type definition.
fn validate_type(
    model: &SemanticModel,
    interner: &StringInterner,
    def_id: DefId,
    sink: &mut DiagnosticSink,
) {
    let def = &model.defs[def_id];

    // Check that specialization targets are actually types (not packages or features)
    for spec in &def.specializations {
        if let Some(target_id) = spec.resolved_def() {
            let target = &model.defs[target_id];
            if target.kind != DefKind::Type {
                let name = interner.resolve(target.name);
                sink.emit(
                    Diagnostic::error(format!("`{}` is a {:?}, not a type", name, target.kind))
                        .with_label(Label::primary(spec.span, "expected a type here")),
                );
            }
        }
    }

    // Check that conjugation target is a type
    if let Some(conj) = &def.conjugation {
        if let Some(target_id) = conj.resolved_def() {
            let target = &model.defs[target_id];
            if target.kind != DefKind::Type {
                let name = interner.resolve(target.name);
                sink.emit(
                    Diagnostic::error(format!(
                        "conjugation target `{}` is a {:?}, not a type",
                        name, target.kind
                    ))
                    .with_label(Label::primary(conj.span, "expected a type here")),
                );
            }
        }
    }

    // Warn when conjugation target has no features
    if let Some(conj) = &def.conjugation {
        if let Some(target_id) = conj.resolved_def() {
            let target = &model.defs[target_id];
            let has_features = target
                .children
                .iter()
                .any(|c| model.defs[*c].kind == DefKind::Feature);
            if !has_features {
                let name = interner.resolve(target.name);
                sink.emit(
                    Diagnostic::warning(format!(
                        "conjugation target `{name}` has no \
                         features; conjugation has no effect"
                    ))
                    .with_label(Label::primary(
                        conj.span,
                        "this type has no features to flip",
                    )),
                );
            }
        }
    }

    // Check for duplicate feature names among own children
    let own_features: Vec<DefId> = def
        .children
        .iter()
        .filter(|&&c| model.defs[c].kind == DefKind::Feature)
        .copied()
        .collect();

    for i in 0..own_features.len() {
        for j in (i + 1)..own_features.len() {
            let a = &model.defs[own_features[i]];
            let b = &model.defs[own_features[j]];
            if a.name == b.name {
                let name = interner.resolve(a.name);
                sink.emit(
                    Diagnostic::error(format!("duplicate feature `{}`", name))
                        .with_label(Label::primary(b.span, "duplicate definition"))
                        .with_label(Label::secondary(a.span, "first defined here")),
                );
            }
        }
    }
}

/// Validate a named conjugation declaration.
fn validate_conjugation_decl(
    model: &SemanticModel,
    interner: &StringInterner,
    def_id: DefId,
    sink: &mut DiagnosticSink,
) {
    let def = &model.defs[def_id];
    let (conj_ref, orig_ref) = match &def.conjugation_decl {
        Some(pair) => pair,
        None => return,
    };

    // Both targets must be types
    if let Some(target_id) = conj_ref.resolved_def() {
        let target = &model.defs[target_id];
        if target.kind != DefKind::Type {
            let name = interner.resolve(target.name);
            sink.emit(
                Diagnostic::error(format!(
                    "conjugated type `{name}` is a {:?}, not a type",
                    target.kind
                ))
                .with_label(Label::primary(conj_ref.span, "expected a type here")),
            );
        }
    }

    if let Some(target_id) = orig_ref.resolved_def() {
        let target = &model.defs[target_id];
        if target.kind != DefKind::Type {
            let name = interner.resolve(target.name);
            sink.emit(
                Diagnostic::error(format!(
                    "original type `{name}` is a {:?}, not a type",
                    target.kind
                ))
                .with_label(Label::primary(orig_ref.span, "expected a type here")),
            );
        }
    }

    // No self-conjugation
    if let (Some(conj_id), Some(orig_id)) = (conj_ref.resolved_def(), orig_ref.resolved_def()) {
        if conj_id == orig_id {
            let name = interner.resolve(model.defs[conj_id].name);
            sink.emit(
                Diagnostic::error(format!("type `{name}` cannot conjugate itself"))
                    .with_label(Label::primary(def.span, "self-conjugation")),
            );
        }
    }

    // Warn if original type has no features
    if let Some(orig_id) = orig_ref.resolved_def() {
        let orig = &model.defs[orig_id];
        let has_features = orig
            .children
            .iter()
            .any(|c| model.defs[*c].kind == DefKind::Feature);
        if !has_features {
            let name = interner.resolve(orig.name);
            sink.emit(
                Diagnostic::warning(format!(
                    "original type `{name}` has no features; \
                     conjugation has no effect"
                ))
                .with_label(Label::primary(
                    orig_ref.span,
                    "this type has no features to flip",
                )),
            );
        }
    }
}

/// Validate a feature definition.
fn validate_feature(
    model: &SemanticModel,
    interner: &StringInterner,
    def_id: DefId,
    sink: &mut DiagnosticSink,
) {
    let def = &model.defs[def_id];

    // Check that type ref points to a type
    if let Some(type_ref) = &def.type_ref {
        if let Some(target_id) = type_ref.resolved_def() {
            let target = &model.defs[target_id];
            if target.kind != DefKind::Type {
                let name = interner.resolve(target.name);
                sink.emit(
                    Diagnostic::error(format!(
                        "feature type `{}` is a {:?}, not a type",
                        name, target.kind
                    ))
                    .with_label(Label::primary(type_ref.span, "expected a type here")),
                );
            }
        }
    }

    // Validate multiplicity bounds
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

    // Check multiplicity consistency with redefined inherited feature
    if let Some(parent_id) = def.parent {
        let parent = &model.defs[parent_id];
        if parent.kind == DefKind::Type {
            // Check if this feature redefines an inherited one
            for inherited_feat in &parent.inherited_features {
                let inherited = &model.defs[inherited_feat.def_id];
                if inherited.name == def.name {
                    // This feature redefines an inherited feature
                    validate_redefinition_multiplicity(
                        model,
                        interner,
                        def_id,
                        inherited_feat.def_id,
                        sink,
                    );
                }
            }
        }
    }
}

/// Validate that a redefined feature doesn't widen multiplicity.
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
        _ => return, // No multiplicity to compare
    };

    // Check: redefining lower bound must be >= original lower bound
    if redef_mult.lower < orig_mult.lower {
        let name = interner.resolve(redef.name);
        sink.emit(
            Diagnostic::warning(format!(
                "redefined feature `{}` narrows lower multiplicity bound from {} to {}",
                name, orig_mult.lower, redef_mult.lower
            ))
            .with_label(Label::primary(redef_mult.span, "redefined here"))
            .with_label(Label::secondary(orig_mult.span, "original multiplicity")),
        );
    }

    // Check: redefining upper bound must be <= original upper bound
    match (redef_mult.upper, orig_mult.upper) {
        (Bound::Unbounded, Bound::Exact(_)) => {
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
        (Bound::Exact(r), Bound::Exact(o)) if r > o => {
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
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kermlc_diagnostics::SourceMap;
    use kermlc_hir::lower_ast;
    use kermlc_intern::StringInterner;
    use kermlc_parser::Parser;
    use kermlc_resolve::{emit_unresolved_errors, resolve_pass};
    use kermlc_typeck::typecheck_pass;

    fn compile_and_validate(input: &str) -> (SemanticModel, DiagnosticSink) {
        let mut interner = StringInterner::new();
        let mut source_map = SourceMap::new();
        let mut sink = DiagnosticSink::new();
        let file_id = source_map.add_file("test.kerml".into(), input.into());
        let parse = Parser::parse(input, file_id, &mut interner, &mut sink);
        let mut model = lower_ast(&parse, &interner, &mut sink);

        // Fixpoint resolve + typecheck
        for _ in 0..10 {
            let r = resolve_pass(&mut model, &interner, &mut sink);
            let t = typecheck_pass(&mut model, &interner, &mut sink);
            if !r && !t {
                break;
            }
        }
        emit_unresolved_errors(&model, &interner, &mut sink);

        // Validate
        validate(&model, &interner, &mut sink);
        (model, sink)
    }

    #[test]
    fn valid_model_passes() {
        let (_model, sink) =
            compile_and_validate("package P { type A {} type B :> A { feature x : A; } }");
        assert!(!sink.has_errors());
    }

    #[test]
    fn invalid_multiplicity_bounds() {
        let (_model, sink) = compile_and_validate("package P { type T { feature x : T [5..2]; } }");
        assert!(sink.has_errors());
    }

    #[test]
    fn specialization_target_must_be_type() {
        let (_model, sink) = compile_and_validate("package P { type A :> P {} }");
        assert!(sink.has_errors());
    }

    #[test]
    fn duplicate_features_detected() {
        let (_model, sink) =
            compile_and_validate("package P { type A { feature x : A; feature x : A; } }");
        assert!(sink.has_errors());
    }

    #[test]
    fn conjugation_target_no_features_warns() {
        let (_model, sink) = compile_and_validate("package P { type Empty {} type B ~ Empty {} }");
        assert!(!sink.has_errors());
        let warnings: Vec<_> = sink
            .diagnostics()
            .iter()
            .filter(|d| d.message.contains("no features"))
            .collect();
        assert!(
            !warnings.is_empty(),
            "should warn about conjugation target with no features"
        );
    }
}
