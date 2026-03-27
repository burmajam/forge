use crate::types::*;
use kermlc_ast;
use kermlc_diagnostics::DiagnosticSink;
use kermlc_intern::StringInterner;

/// Lower a ParseResult (AST) into a SemanticModel (HIR).
pub fn lower_ast(
    parse: &kermlc_parser::ParseResult,
    interner: &mut StringInterner,
    _sink: &mut DiagnosticSink,
) -> SemanticModel {
    let mut model = SemanticModel::new();
    let mut ctx = LowerCtx {
        model: &mut model,
        parse,
        interner,
    };

    // Lower top-level packages
    for &pkg_id in &parse.source_file.packages {
        let def_id = ctx.lower_package(pkg_id);
        ctx.model.roots.push(def_id);
    }

    // Lower top-level members
    for member in &parse.source_file.members {
        for def_id in ctx.lower_member(member) {
            ctx.model.roots.push(def_id);
        }
    }

    model
}

struct LowerCtx<'a> {
    model: &'a mut SemanticModel,
    parse: &'a kermlc_parser::ParseResult,
    interner: &'a mut StringInterner,
}

impl<'a> LowerCtx<'a> {
    fn lower_package(&mut self, pkg_id: kermlc_ast::PackageId) -> DefId {
        let pkg = &self.parse.packages[pkg_id];
        let mut def = Def::new(pkg.name, DefKind::Package, pkg.span);

        // Lower imports
        for import in &pkg.imports {
            def.imports.push(Import {
                path: NameRef::unresolved(import.path.segments.clone(), import.path.span),
                is_wildcard: import.is_wildcard,
                span: import.span,
            });
        }

        let def_id = self.model.alloc_def(def);

        // Lower members
        for member in &pkg.members {
            for child_id in self.lower_member(member) {
                self.model.add_child(def_id, child_id);
            }
        }

        def_id
    }

    /// Lower a member, returning the primary DefId plus any
    /// synthesized siblings (e.g. anonymous conjugated types).
    fn lower_member(&mut self, member: &kermlc_ast::Member) -> Vec<DefId> {
        match member {
            kermlc_ast::Member::Package(id) => {
                vec![self.lower_package(*id)]
            }
            kermlc_ast::Member::Type(id) => {
                vec![self.lower_type(*id)]
            }
            kermlc_ast::Member::Feature(id) => self.lower_feature(*id),
            kermlc_ast::Member::Conjugation(id) => {
                vec![self.lower_conjugation_decl(*id)]
            }
        }
    }

    fn lower_type(&mut self, type_id: kermlc_ast::TypeDeclId) -> DefId {
        let ty = &self.parse.types[type_id];
        let mut def = Def::new(ty.name, DefKind::Type, ty.span);

        // Lower specializations as unresolved name refs
        for spec in &ty.specializations {
            def.specializations
                .push(NameRef::unresolved(spec.segments.clone(), spec.span));
        }

        // Lower conjugation
        if let Some(conj) = &ty.conjugation {
            def.conjugation = Some(NameRef::unresolved(conj.segments.clone(), conj.span));
        }

        let def_id = self.model.alloc_def(def);

        // Lower nested members
        for member in &ty.members {
            for child_id in self.lower_member(member) {
                self.model.add_child(def_id, child_id);
            }
        }

        def_id
    }

    /// Lower a feature declaration, returning the feature DefId
    /// plus any synthesized anonymous types as siblings.
    fn lower_feature(&mut self, feat_id: kermlc_ast::FeatureDeclId) -> Vec<DefId> {
        let feat = &self.parse.features[feat_id];
        let mut def = Def::new(feat.name, DefKind::Feature, feat.span);
        let mut extra_siblings = Vec::new();

        // Lower direction
        def.direction = feat.direction;

        // Lower type reference
        match &feat.type_ref {
            Some(kermlc_ast::TypeExpr::Named(qn)) => {
                def.type_ref = Some(NameRef::unresolved(qn.segments.clone(), qn.span));
            }
            Some(kermlc_ast::TypeExpr::Conjugated(qn, span)) => {
                let anon_id = self.synthesize_conjugated_type(qn, *span);
                def.type_ref = Some(NameRef {
                    segments: vec![],
                    span: *span,
                    resolution: ResolutionState::Resolved(anon_id),
                });
                extra_siblings.push(anon_id);
            }
            Some(kermlc_ast::TypeExpr::Chain(_)) => {
                // Future: chain-as-type lowering
            }
            None => {}
        }

        // Lower conjugation
        if let Some(conj) = &feat.conjugation {
            def.conjugation = Some(NameRef::unresolved(conj.segments.clone(), conj.span));
        }

        // Lower feature chain
        if let Some(chain) = &feat.chain {
            for seg in &chain.segments {
                def.chain_segments
                    .push(NameRef::unresolved(seg.segments.clone(), seg.span));
            }
        }

        // Lower multiplicity
        if let Some(mult) = &feat.multiplicity {
            def.multiplicity = Some(lower_multiplicity(mult));
        }

        let feat_id = self.model.alloc_def(def);
        let mut result = vec![feat_id];
        result.append(&mut extra_siblings);
        result
    }

    fn lower_conjugation_decl(&mut self, conj_id: kermlc_ast::ConjugationDeclId) -> DefId {
        let conj = &self.parse.conjugations[conj_id];
        let mut def = Def::new(conj.name, DefKind::Conjugation, conj.span);

        let conjugated = NameRef::unresolved(
            conj.conjugated_type.segments.clone(),
            conj.conjugated_type.span,
        );
        let original =
            NameRef::unresolved(conj.original_type.segments.clone(), conj.original_type.span);
        def.conjugation_decl = Some((conjugated, original));

        self.model.alloc_def(def)
    }

    fn synthesize_conjugated_type(
        &mut self,
        original: &kermlc_ast::QualifiedName,
        span: kermlc_diagnostics::Span,
    ) -> DefId {
        let last_seg = *original.segments.last().expect("empty qualified name");
        let orig_name = self.interner.resolve(last_seg);
        let synth_name = self.interner.intern(&format!("~{orig_name}"));

        let mut anon_def = Def::new(synth_name, DefKind::Type, span);
        anon_def.conjugation = Some(NameRef::unresolved(
            original.segments.clone(),
            original.span,
        ));
        self.model.alloc_def(anon_def)
    }
}

fn lower_multiplicity(mult: &kermlc_ast::Multiplicity) -> HirMultiplicity {
    let lower = mult
        .lower
        .as_ref()
        .map(lower_expr_to_bound)
        .unwrap_or(MultBound::Exact(0));

    let upper = mult
        .upper
        .as_ref()
        .map(lower_expr_to_bound)
        .unwrap_or_else(|| lower.clone());

    HirMultiplicity {
        lower,
        upper,
        span: mult.span,
    }
}

fn lower_expr_to_bound(expr: &kermlc_ast::Expr) -> MultBound {
    match expr {
        kermlc_ast::Expr::IntLiteral { value, .. } => MultBound::Exact(*value),
        kermlc_ast::Expr::Star { .. } => MultBound::Unbounded,
        kermlc_ast::Expr::Name { name } => {
            MultBound::Ref(NameRef::unresolved(name.segments.clone(), name.span))
        }
        kermlc_ast::Expr::BinOp { .. } => {
            unreachable!("BinOp cannot appear in multiplicity bounds per KerML spec")
        }
    }
}

// We need kermlc_parser as a dependency for ParseResult
// This will be added to Cargo.toml

#[cfg(test)]
mod tests {
    use super::*;
    use kermlc_diagnostics::{DiagnosticSink, SourceMap};
    use kermlc_intern::StringInterner;
    use kermlc_parser::Parser;

    fn lower(input: &str) -> (SemanticModel, StringInterner, DiagnosticSink) {
        let mut interner = StringInterner::new();
        let mut source_map = SourceMap::new();
        let mut sink = DiagnosticSink::new();
        let file_id = source_map.add_file("test.kerml".into(), input.into());
        let parse = Parser::parse(input, file_id, &mut interner, &mut sink);
        let model = lower_ast(&parse, &mut interner, &mut sink);
        (model, interner, sink)
    }

    #[test]
    fn lower_creates_package_def() {
        let (model, interner, sink) = lower("package Foo {}");
        assert!(!sink.has_errors());
        assert_eq!(model.roots.len(), 1);
        let root = &model.defs[model.roots[0]];
        assert_eq!(root.kind, DefKind::Package);
        assert_eq!(interner.resolve(root.name), "Foo");
    }

    #[test]
    fn lower_creates_type_with_unresolved_specialization() {
        let (model, interner, sink) = lower("package P { type Car :> Vehicle {} }");
        assert!(!sink.has_errors());

        let pkg = &model.defs[model.roots[0]];
        assert_eq!(pkg.children.len(), 1);

        let car = &model.defs[pkg.children[0]];
        assert_eq!(car.kind, DefKind::Type);
        assert_eq!(interner.resolve(car.name), "Car");
        assert_eq!(car.specializations.len(), 1);
        assert_eq!(
            car.specializations[0].resolution,
            ResolutionState::Unresolved
        );
    }

    #[test]
    fn lower_feature_direction() {
        let (model, interner, sink) = lower("package P { type T { in feature f : Integer; } }");
        assert!(!sink.has_errors());

        let pkg = &model.defs[model.roots[0]];
        let ty = &model.defs[pkg.children[0]];
        let feat = &model.defs[ty.children[0]];
        assert_eq!(feat.kind, DefKind::Feature);
        assert_eq!(interner.resolve(feat.name), "f");
        assert_eq!(feat.direction, Some(FeatureDirection::In));
    }

    #[test]
    fn lower_feature_conjugation() {
        let (model, interner, sink) =
            lower("package P { type T { in feature f; } feature g ~ T::f; }");
        assert!(!sink.has_errors(), "errors: {:?}", sink.diagnostics());

        let pkg = &model.defs[model.roots[0]];
        let g = &model.defs[pkg.children[1]];
        assert_eq!(g.kind, DefKind::Feature);
        assert_eq!(interner.resolve(g.name), "g");
        assert!(
            g.conjugation.is_some(),
            "feature g should have conjugation ref"
        );
        let conj = g.conjugation.as_ref().unwrap();
        assert_eq!(
            conj.resolution,
            ResolutionState::Unresolved,
            "should be unresolved at lowering time"
        );
    }

    #[test]
    fn lower_creates_feature_with_type_ref() {
        let (model, interner, sink) = lower("package P { type T { feature x : Integer [0..1]; } }");
        assert!(!sink.has_errors());

        let pkg = &model.defs[model.roots[0]];
        let ty = &model.defs[pkg.children[0]];
        let feat = &model.defs[ty.children[0]];
        assert_eq!(feat.kind, DefKind::Feature);
        assert_eq!(interner.resolve(feat.name), "x");
        assert!(feat.type_ref.is_some());
        assert!(feat.multiplicity.is_some());
        let mult = feat.multiplicity.as_ref().unwrap();
        assert!(matches!(mult.lower, MultBound::Exact(0)));
        assert!(matches!(mult.upper, MultBound::Exact(1)));
    }

    #[test]
    fn lower_inline_conjugated_type_ref() {
        let (model, interner, sink) =
            lower("package P { type T { in feature f : T; } type U { feature g : ~T; } }");
        assert!(!sink.has_errors(), "errors: {:?}", sink.diagnostics());

        let pkg = &model.defs[model.roots[0]];
        let u_id = pkg.children[1];
        let g_id = model.defs[u_id].children[0];
        let g = &model.defs[g_id];

        assert_eq!(interner.resolve(g.name), "g");
        assert!(g.type_ref.is_some(), "g should have type_ref");
        let type_ref = g.type_ref.as_ref().unwrap();
        assert!(
            type_ref.is_resolved(),
            "type_ref should be pre-resolved to anonymous type"
        );

        let anon_id = type_ref.resolved_def().unwrap();
        let anon = &model.defs[anon_id];
        assert_eq!(anon.kind, DefKind::Type);
        assert_eq!(interner.resolve(anon.name), "~T");
        assert!(
            anon.conjugation.is_some(),
            "anonymous type should have conjugation"
        );
        assert_eq!(
            anon.conjugation.as_ref().unwrap().resolution,
            ResolutionState::Unresolved,
            "conjugation target should be unresolved at lowering time"
        );
    }

    #[test]
    fn lower_multiplicity_with_feature_ref() {
        let (model, interner, sink) =
            lower("package P { type T { feature n : T; feature x : T [1..n]; } }");
        assert!(!sink.has_errors(), "errors: {:?}", sink.diagnostics());

        let pkg = &model.defs[model.roots[0]];
        let ty = &model.defs[pkg.children[0]];
        let x = &model.defs[ty.children[1]];
        let mult = x.multiplicity.as_ref().expect("x should have multiplicity");

        assert!(
            matches!(mult.lower, MultBound::Exact(1)),
            "lower should be Exact(1), got {:?}",
            mult.lower
        );
        assert!(
            matches!(mult.upper, MultBound::Ref(_)),
            "upper should be Ref, got {:?}",
            mult.upper
        );
        if let MultBound::Ref(ref name_ref) = mult.upper {
            assert_eq!(name_ref.resolution, ResolutionState::Unresolved);
            assert_eq!(interner.resolve(name_ref.segments[0]), "n");
        }
    }

    #[test]
    fn lower_multiplicity_exact_unchanged() {
        let (model, _interner, sink) = lower("package P { type T { feature x : T [0..1]; } }");
        assert!(!sink.has_errors());
        let pkg = &model.defs[model.roots[0]];
        let ty = &model.defs[pkg.children[0]];
        let feat = &model.defs[ty.children[0]];
        let mult = feat.multiplicity.as_ref().unwrap();
        assert!(matches!(mult.lower, MultBound::Exact(0)));
        assert!(matches!(mult.upper, MultBound::Exact(1)));
    }

    #[test]
    fn lower_multiplicity_star_unchanged() {
        let (model, _interner, sink) = lower("package P { type T { feature x : T [0..*]; } }");
        assert!(!sink.has_errors());
        let pkg = &model.defs[model.roots[0]];
        let ty = &model.defs[pkg.children[0]];
        let feat = &model.defs[ty.children[0]];
        let mult = feat.multiplicity.as_ref().unwrap();
        assert!(matches!(mult.lower, MultBound::Exact(0)));
        assert!(matches!(mult.upper, MultBound::Unbounded));
    }
}
