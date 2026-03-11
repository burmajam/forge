use kermlc_diagnostics::{DiagnosticSink, SourceMap};
use kermlc_hir::{
    add_implicit_specializations, load_stdlib, lower_ast, FeatureDirection, InheritanceKind,
    SemanticModel,
};
use kermlc_intern::StringInterner;
use kermlc_resolve::{detect_specialization_cycles, emit_unresolved_errors, resolve_pass};
use kermlc_serial_json::serialize_to_json;
use kermlc_typeck::typecheck_pass;
use kermlc_validate::validate;

/// Compile a KerML source string through the full pipeline.
fn compile_source(source: &str) -> CompileResult {
    let mut interner = StringInterner::new();
    let mut source_map = SourceMap::new();
    let mut sink = DiagnosticSink::new();
    let file_id = source_map.add_file("test.kerml".into(), source.into());

    let parse = kermlc_parser::Parser::parse(source, file_id, &mut interner, &mut sink);
    let mut model = lower_ast(&parse, &interner, &mut sink);
    let stdlib = load_stdlib(&mut model, &mut interner);
    add_implicit_specializations(&mut model, &stdlib);

    // Fixpoint loop
    for _ in 0..100 {
        let r = resolve_pass(&mut model, &interner, &mut sink);
        let t = typecheck_pass(&mut model, &interner, &mut sink);
        if !r && !t {
            break;
        }
    }
    emit_unresolved_errors(&model, &interner, &mut sink);
    detect_specialization_cycles(&model, &interner, &mut sink);
    validate(&model, &interner, &mut sink);

    CompileResult {
        model,
        interner,
        sink,
    }
}

/// Get the fixtures directory based on CARGO_MANIFEST_DIR.
fn fixtures_dir() -> std::path::PathBuf {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    std::path::PathBuf::from(manifest_dir)
        .join("tests")
        .join("fixtures")
}

/// Compile a KerML file from disk through the full pipeline.
fn compile_file(path: &std::path::Path) -> CompileResult {
    let source = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("could not read {}: {}", path.display(), e));
    compile_source(&source)
}

struct CompileResult {
    model: SemanticModel,
    interner: StringInterner,
    sink: DiagnosticSink,
}

// ── Valid fixture tests ──────────────────────────────────────────────

#[test]
fn valid_simple_package() {
    let result = compile_file(&fixtures_dir().join("valid/simple_package.kerml"));
    assert!(
        !result.sink.has_errors(),
        "Errors in simple_package.kerml: {:?}",
        result.sink.diagnostics()
    );
}

#[test]
fn valid_specialization() {
    let result = compile_file(&fixtures_dir().join("valid/specialization.kerml"));
    assert!(
        !result.sink.has_errors(),
        "Errors in specialization.kerml: {:?}",
        result.sink.diagnostics()
    );
}

#[test]
fn valid_conjugation() {
    let result = compile_file(&fixtures_dir().join("valid/conjugation.kerml"));
    assert!(
        !result.sink.has_errors(),
        "Errors in conjugation.kerml: {:?}",
        result.sink.diagnostics()
    );

    // Find Sink type and verify inherited features
    let pkg = result.model.roots[0];
    let sink_id = result.model.defs[pkg]
        .children
        .iter()
        .find(|&&c| result.interner.resolve(result.model.defs[c].name) == "Sink")
        .copied()
        .expect("Sink type not found");

    let sink_def = &result.model.defs[sink_id];
    assert_eq!(
        sink_def.inherited_features.len(),
        4,
        "Sink should inherit 4 features from Source"
    );

    for inh in &sink_def.inherited_features {
        assert_eq!(inh.kind, InheritanceKind::Conjugation);
        let feat_name = result.interner.resolve(result.model.defs[inh.def_id].name);
        match feat_name {
            "input" => assert_eq!(
                inh.direction_override,
                Some(FeatureDirection::Out),
                "in should flip to out"
            ),
            "output" => assert_eq!(
                inh.direction_override,
                Some(FeatureDirection::In),
                "out should flip to in"
            ),
            "control" => assert_eq!(
                inh.direction_override,
                Some(FeatureDirection::InOut),
                "inout stays inout"
            ),
            "data" => assert_eq!(inh.direction_override, None, "no direction stays None"),
            other => panic!("unexpected feature: {other}"),
        }
    }
}

#[test]
fn valid_conjugation_chained() {
    let result = compile_file(&fixtures_dir().join("valid/conjugation_chained.kerml"));
    assert!(
        !result.sink.has_errors(),
        "Errors in conjugation_chained.kerml: {:?}",
        result.sink.diagnostics()
    );

    let pkg = result.model.roots[0];
    let children = &result.model.defs[pkg].children;

    // Find B and C by name
    let find_type = |name: &str| {
        children
            .iter()
            .find(|&&c| result.interner.resolve(result.model.defs[c].name) == name)
            .copied()
            .unwrap_or_else(|| panic!("{name} not found"))
    };
    let b_id = find_type("B");
    let c_id = find_type("C");

    // B conjugates A: in->out, out->in
    let b_def = &result.model.defs[b_id];
    assert_eq!(b_def.inherited_features.len(), 2);
    for inh in &b_def.inherited_features {
        let name = result.interner.resolve(result.model.defs[inh.def_id].name);
        match name {
            "f" => assert_eq!(
                inh.direction_override,
                Some(FeatureDirection::Out),
                "B.f: in should flip to out"
            ),
            "g" => assert_eq!(
                inh.direction_override,
                Some(FeatureDirection::In),
                "B.g: out should flip to in"
            ),
            other => panic!("unexpected feature: {other}"),
        }
    }

    // C conjugates B: double flip back to original
    let c_def = &result.model.defs[c_id];
    assert_eq!(c_def.inherited_features.len(), 2);
    for inh in &c_def.inherited_features {
        let name = result.interner.resolve(result.model.defs[inh.def_id].name);
        match name {
            "f" => assert_eq!(
                inh.direction_override,
                Some(FeatureDirection::In),
                "C.f: double flip back to in"
            ),
            "g" => assert_eq!(
                inh.direction_override,
                Some(FeatureDirection::Out),
                "C.g: double flip back to out"
            ),
            other => panic!("unexpected feature: {other}"),
        }
    }
}

#[test]
fn valid_conjugation_named() {
    let result = compile_file(&fixtures_dir().join("valid/conjugation_named.kerml"));
    assert!(
        !result.sink.has_errors(),
        "Errors in conjugation_named.kerml: {:?}",
        result.sink.diagnostics()
    );

    // Find Sink type and verify inherited features
    let pkg = result.model.roots[0];
    let sink_id = result.model.defs[pkg]
        .children
        .iter()
        .find(|&&c| result.interner.resolve(result.model.defs[c].name) == "Sink")
        .copied()
        .expect("Sink type not found");

    let sink_def = &result.model.defs[sink_id];
    assert_eq!(
        sink_def.inherited_features.len(),
        4,
        "Sink should inherit 4 features from Source via named conjugation"
    );

    for inh in &sink_def.inherited_features {
        assert_eq!(inh.kind, InheritanceKind::Conjugation);
        let feat_name = result.interner.resolve(result.model.defs[inh.def_id].name);
        match feat_name {
            "input" => assert_eq!(
                inh.direction_override,
                Some(FeatureDirection::Out),
                "in should flip to out"
            ),
            "output" => assert_eq!(
                inh.direction_override,
                Some(FeatureDirection::In),
                "out should flip to in"
            ),
            "control" => assert_eq!(
                inh.direction_override,
                Some(FeatureDirection::InOut),
                "inout stays inout"
            ),
            "data" => assert_eq!(inh.direction_override, None, "no direction stays None"),
            other => panic!("unexpected feature: {other}"),
        }
    }

    // Verify the conjugation def itself exists
    let conj_id = result.model.defs[pkg]
        .children
        .iter()
        .find(|&&c| result.interner.resolve(result.model.defs[c].name) == "c1")
        .copied()
        .expect("conjugation c1 not found");
    assert_eq!(
        result.model.defs[conj_id].kind,
        kermlc_hir::DefKind::Conjugation
    );
}

#[test]
fn valid_feature_conjugation() {
    let result = compile_file(&fixtures_dir().join("valid/feature_conjugation.kerml"));
    assert!(
        !result.sink.has_errors(),
        "Errors in feature_conjugation.kerml: {:?}",
        result.sink.diagnostics()
    );

    let pkg = result.model.roots[0];
    let children = &result.model.defs[pkg].children;

    // Find feature g (top-level feature in package, after Source type)
    let g_id = children
        .iter()
        .find(|&&c| result.interner.resolve(result.model.defs[c].name) == "g")
        .copied()
        .expect("feature g not found");

    let g_def = &result.model.defs[g_id];
    assert_eq!(g_def.kind, kermlc_hir::DefKind::Feature);
    assert!(g_def.conjugation.is_some(), "g should have conjugation ref");
    assert!(
        g_def.conjugation.as_ref().unwrap().is_resolved(),
        "g's conjugation should be resolved"
    );

    // Find Tanks type
    let tanks_id = children
        .iter()
        .find(|&&c| result.interner.resolve(result.model.defs[c].name) == "Tanks")
        .copied()
        .expect("Tanks type not found");

    let tanks_children = &result.model.defs[tanks_id].children;

    // Find fuelOutPort
    let out_port_id = tanks_children
        .iter()
        .find(|&&c| result.interner.resolve(result.model.defs[c].name) == "fuelOutPort")
        .copied()
        .expect("fuelOutPort not found");

    let out_port = &result.model.defs[out_port_id];
    assert!(
        out_port.conjugation.is_some(),
        "fuelOutPort should have conjugation"
    );
    assert!(
        out_port.conjugation.as_ref().unwrap().is_resolved(),
        "fuelOutPort conjugation should be resolved"
    );
}

#[test]
fn valid_feature_chain() {
    let result = compile_file(&fixtures_dir().join("valid/feature_chain.kerml"));
    assert!(
        !result.sink.has_errors(),
        "Errors in feature_chain.kerml: {:?}",
        result.sink.diagnostics()
    );
}

#[test]
fn valid_imports() {
    let result = compile_file(&fixtures_dir().join("valid/imports.kerml"));
    assert!(
        !result.sink.has_errors(),
        "Errors in imports.kerml: {:?}",
        result.sink.diagnostics()
    );
}

#[test]
fn valid_direction() {
    let result = compile_file(&fixtures_dir().join("valid/direction.kerml"));
    assert!(
        !result.sink.has_errors(),
        "Errors in direction.kerml: {:?}",
        result.sink.diagnostics()
    );
}

// ── Invalid fixture tests ────────────────────────────────────────────

#[test]
fn invalid_unresolved_type() {
    let result = compile_file(&fixtures_dir().join("invalid/unresolved_type.kerml"));
    assert!(
        result.sink.has_errors(),
        "Expected errors in unresolved_type.kerml"
    );
}

#[test]
fn invalid_missing_brace() {
    let result = compile_file(&fixtures_dir().join("invalid/missing_brace.kerml"));
    assert!(
        result.sink.has_errors(),
        "Expected errors in missing_brace.kerml"
    );
}

#[test]
fn invalid_feature_conjugates_type() {
    let result = compile_file(&fixtures_dir().join("invalid/feature_conjugates_type.kerml"));
    assert!(
        result.sink.has_errors(),
        "Feature conjugating a Type should produce an error"
    );
}

// ── Pipeline behavior tests ─────────────────────────────────────────

#[test]
fn json_serialization_produces_valid_json() {
    let result = compile_source("package Foo { type Bar {} }");
    assert!(!result.sink.has_errors());

    let json = serialize_to_json(&result.model, &result.interner);
    let parsed: Result<Vec<serde_json::Value>, _> = serde_json::from_str(&json);
    assert!(parsed.is_ok(), "Invalid JSON output: {}", json);
    let elements = parsed.unwrap();
    assert!(!elements.is_empty());
}

#[test]
fn cross_package_qualified_resolution() {
    let result = compile_source(
        r#"
        package A { type X {} }
        package B { type Y :> A::X {} }
        "#,
    );
    assert!(
        !result.sink.has_errors(),
        "Cross-package qualified name should resolve: {:?}",
        result.sink.diagnostics()
    );
}

#[test]
fn specialization_chain_inherits_features() {
    let result = compile_source(
        r#"
        package P {
            type A { feature x : A; }
            type B :> A {}
            type C :> B {}
        }
        "#,
    );
    assert!(!result.sink.has_errors());

    // Verify C inherits from the chain
    let pkg = result.model.roots[0];
    let c_id = result.model.defs[pkg].children[2];
    assert!(
        !result.model.defs[c_id].inherited_features.is_empty(),
        "C should inherit features through specialization chain A -> B -> C"
    );
}

#[test]
fn validation_catches_bad_multiplicity() {
    let result = compile_source("package P { type T { feature x : T [5..2]; } }");
    assert!(
        result.sink.has_errors(),
        "Should catch lower > upper multiplicity"
    );
}
