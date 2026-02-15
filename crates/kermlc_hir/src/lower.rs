use crate::types::*;
use kermlc_ast;
use kermlc_diagnostics::DiagnosticSink;
use kermlc_intern::StringInterner;

/// Lower a ParseResult (AST) into a SemanticModel (HIR).
pub fn lower_ast(
    parse: &kermlc_parser::ParseResult,
    interner: &StringInterner,
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
        let def_id = ctx.lower_member(member);
        ctx.model.roots.push(def_id);
    }

    model
}

struct LowerCtx<'a> {
    model: &'a mut SemanticModel,
    parse: &'a kermlc_parser::ParseResult,
    #[allow(dead_code)]
    interner: &'a StringInterner,
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
            let child_id = self.lower_member(member);
            self.model.add_child(def_id, child_id);
        }

        def_id
    }

    fn lower_member(&mut self, member: &kermlc_ast::Member) -> DefId {
        match member {
            kermlc_ast::Member::Package(id) => self.lower_package(*id),
            kermlc_ast::Member::Type(id) => self.lower_type(*id),
            kermlc_ast::Member::Feature(id) => self.lower_feature(*id),
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
            let child_id = self.lower_member(member);
            self.model.add_child(def_id, child_id);
        }

        def_id
    }

    fn lower_feature(&mut self, feat_id: kermlc_ast::FeatureDeclId) -> DefId {
        let feat = &self.parse.features[feat_id];
        let mut def = Def::new(feat.name, DefKind::Feature, feat.span);

        // Lower type reference
        if let Some(type_ref) = &feat.type_ref {
            def.type_ref = Some(NameRef::unresolved(
                type_ref.segments.clone(),
                type_ref.span,
            ));
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

        self.model.alloc_def(def)
    }
}

fn lower_multiplicity(mult: &kermlc_ast::Multiplicity) -> HirMultiplicity {
    let lower = mult
        .lower
        .as_ref()
        .map(|e| eval_const_expr(e))
        .unwrap_or(0);

    let upper = mult
        .upper
        .as_ref()
        .map(|e| match e {
            kermlc_ast::Expr::Star { .. } => Bound::Unbounded,
            _ => Bound::Exact(eval_const_expr(e)),
        })
        .unwrap_or(Bound::Exact(lower));

    HirMultiplicity {
        lower,
        upper,
        span: mult.span,
    }
}

fn eval_const_expr(expr: &kermlc_ast::Expr) -> u64 {
    match expr {
        kermlc_ast::Expr::IntLiteral { value, .. } => *value,
        _ => 0,
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
        let model = lower_ast(&parse, &interner, &mut sink);
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
    fn lower_creates_feature_with_type_ref() {
        let (model, interner, sink) =
            lower("package P { type T { feature x : Integer [0..1]; } }");
        assert!(!sink.has_errors());

        let pkg = &model.defs[model.roots[0]];
        let ty = &model.defs[pkg.children[0]];
        let feat = &model.defs[ty.children[0]];
        assert_eq!(feat.kind, DefKind::Feature);
        assert_eq!(interner.resolve(feat.name), "x");
        assert!(feat.type_ref.is_some());
        assert!(feat.multiplicity.is_some());
        let mult = feat.multiplicity.as_ref().unwrap();
        assert_eq!(mult.lower, 0);
        assert_eq!(mult.upper, Bound::Exact(1));
    }
}
