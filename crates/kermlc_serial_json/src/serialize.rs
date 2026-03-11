use kermlc_hir::{DefKind, FeatureDirection, InheritanceKind, SemanticModel};
use kermlc_intern::StringInterner;
use serde_json::{json, Value};

/// Serialize a SemanticModel to JSON-LD format following the SysML v2 API structure.
pub fn serialize_to_json(model: &SemanticModel, interner: &StringInterner) -> String {
    let elements = build_elements(model, interner);
    serde_json::to_string_pretty(&elements).unwrap_or_else(|_| "[]".to_string())
}

/// Build JSON-LD element array from the semantic model.
fn build_elements(model: &SemanticModel, interner: &StringInterner) -> Vec<Value> {
    let mut elements = Vec::new();

    for (def_id, def) in model.defs.iter() {
        let at_type = match def.kind {
            DefKind::Package => "Package",
            DefKind::Type => "Type",
            DefKind::Feature => "Feature",
            DefKind::Conjugation => "Conjugation",
        };

        let name = interner.resolve(def.name);
        let id = format!("{}-{}", at_type.to_lowercase(), def_id.raw());

        let mut element = json!({
            "@type": at_type,
            "@id": id,
            "name": name,
        });

        // Add owned members
        if !def.children.is_empty() {
            let member_refs: Vec<Value> = def
                .children
                .iter()
                .map(|&child_id| {
                    let child = &model.defs[child_id];
                    let child_type = match child.kind {
                        DefKind::Package => "package",
                        DefKind::Type => "type",
                        DefKind::Feature => "feature",
                        DefKind::Conjugation => "conjugation",
                    };
                    json!({
                        "@id": format!("{}-{}", child_type, child_id.raw()),
                    })
                })
                .collect();
            element["ownedMember"] = Value::Array(member_refs);
        }

        // Add specializations for types
        if def.kind == DefKind::Type && !def.specializations.is_empty() {
            let specs: Vec<Value> = def
                .specializations
                .iter()
                .filter_map(|s| {
                    s.resolved_def().map(|target_id| {
                        json!({
                            "@type": "Specialization",
                            "general": {
                                "@id": format!("type-{}", target_id.raw()),
                            },
                            "specific": {
                                "@id": id.clone(),
                            },
                        })
                    })
                })
                .collect();
            if !specs.is_empty() {
                element["ownedSpecialization"] = Value::Array(specs);
            }
        }

        // Add conjugation for types
        if let Some(conj) = &def.conjugation {
            if let Some(target_id) = conj.resolved_def() {
                element["ownedConjugator"] = json!({
                    "@type": "Conjugation",
                    "conjugatedType": {
                        "@id": format!("type-{}", target_id.raw()),
                    },
                    "originalType": {
                        "@id": id.clone(),
                    },
                });
            }
        }

        // Add inherited features with direction info
        if !def.inherited_features.is_empty() {
            let inherited_refs: Vec<Value> = def
                .inherited_features
                .iter()
                .map(|inh| {
                    let kind_str = match inh.kind {
                        InheritanceKind::Specialization => "specialization",
                        InheritanceKind::Conjugation => "conjugation",
                    };
                    let mut obj = json!({
                        "@id": format!(
                            "feature-{}",
                            inh.def_id.raw()
                        ),
                        "inheritanceKind": kind_str,
                    });
                    let effective_dir = inh.direction_override.or(model.defs[inh.def_id].direction);
                    if let Some(dir) = effective_dir {
                        let dir_str = match dir {
                            FeatureDirection::In => "in",
                            FeatureDirection::Out => "out",
                            FeatureDirection::InOut => "inout",
                        };
                        obj.as_object_mut()
                            .unwrap()
                            .insert("direction".to_string(), json!(dir_str));
                    }
                    obj
                })
                .collect();
            element["inheritedFeature"] = json!(inherited_refs);
        }

        // Add conjugation declaration refs
        if let Some((ref conj, ref orig)) = def.conjugation_decl {
            if let Some(conj_id) = conj.resolved_def() {
                element["conjugatedType"] = json!({
                    "@id": format!("type-{}", conj_id.raw()),
                });
            }
            if let Some(orig_id) = orig.resolved_def() {
                element["originalType"] = json!({
                    "@id": format!("type-{}", orig_id.raw()),
                });
            }
        }

        // Add typing for features
        if def.kind == DefKind::Feature {
            if let Some(type_ref) = &def.type_ref {
                if let Some(target_id) = type_ref.resolved_def() {
                    element["ownedTyping"] = json!([{
                        "@type": "FeatureTyping",
                        "type": {
                            "@id": format!("type-{}", target_id.raw()),
                        },
                    }]);
                }
            }

            // Add multiplicity
            if let Some(mult) = &def.multiplicity {
                element["ownedMultiplicity"] = json!({
                    "@type": "MultiplicityRange",
                    "lowerBound": mult_bound_to_json(
                        &mult.lower, model, interner,
                    ),
                    "upperBound": mult_bound_to_json(
                        &mult.upper, model, interner,
                    ),
                });
            }

            // Add direction
            if let Some(dir) = &def.direction {
                element["direction"] = json!(match dir {
                    FeatureDirection::In => "in",
                    FeatureDirection::Out => "out",
                    FeatureDirection::InOut => "inout",
                });
            }
        }

        // Add owner reference
        if let Some(parent_id) = def.parent {
            let parent = &model.defs[parent_id];
            let parent_type = match parent.kind {
                DefKind::Package => "package",
                DefKind::Type => "type",
                DefKind::Feature => "feature",
                DefKind::Conjugation => "conjugation",
            };
            element["owner"] = json!({
                "@id": format!("{}-{}", parent_type, parent_id.raw()),
            });
        }

        elements.push(element);
    }

    elements
}

fn mult_bound_to_json(
    bound: &kermlc_hir::MultBound,
    model: &SemanticModel,
    interner: &StringInterner,
) -> Value {
    match bound {
        kermlc_hir::MultBound::Exact(n) => json!(n),
        kermlc_hir::MultBound::Unbounded => json!("*"),
        kermlc_hir::MultBound::Ref(name_ref) => {
            match name_ref.resolved_def() {
                Some(def_id) => {
                    let name =
                        interner.resolve(model.defs[def_id].name);
                    json!({
                        "@type": "FeatureReferenceExpression",
                        "reference": name,
                    })
                }
                None => json!({
                    "@type": "FeatureReferenceExpression",
                    "reference": null,
                }),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kermlc_diagnostics::{DiagnosticSink, SourceMap};
    use kermlc_hir::lower_ast;
    use kermlc_intern::StringInterner;
    use kermlc_parser::Parser;
    use kermlc_resolve::{emit_unresolved_errors, resolve_pass};
    use kermlc_typeck::typecheck_pass;

    fn compile_and_serialize(input: &str) -> String {
        let mut interner = StringInterner::new();
        let mut source_map = SourceMap::new();
        let mut sink = DiagnosticSink::new();
        let file_id = source_map.add_file("test.kerml".into(), input.into());
        let parse = Parser::parse(input, file_id, &mut interner, &mut sink);
        let mut model = lower_ast(&parse, &mut interner, &mut sink);

        for _ in 0..10 {
            let r = resolve_pass(&mut model, &interner, &mut sink);
            let t = typecheck_pass(&mut model, &interner, &mut sink);
            if !r && !t {
                break;
            }
        }
        emit_unresolved_errors(&model, &interner, &mut sink);

        serialize_to_json(&model, &interner)
    }

    #[test]
    fn serialize_simple_package() {
        let json = compile_and_serialize("package Foo { type Bar {} }");
        let value: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap();
        assert!(!value.is_empty());
        assert_eq!(value[0]["@type"], "Package");
        assert_eq!(value[0]["name"], "Foo");
    }

    #[test]
    fn serialize_type_with_specialization() {
        let json = compile_and_serialize("package P { type A {} type B :> A {} }");
        let value: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap();

        // Find B (should have ownedSpecialization)
        let b_elem = value.iter().find(|e| e["name"] == "B").unwrap();
        assert_eq!(b_elem["@type"], "Type");
        assert!(b_elem.get("ownedSpecialization").is_some());
    }

    #[test]
    fn serialize_feature_with_typing() {
        let json =
            compile_and_serialize("package P { type A {} type B { feature x : A [0..1]; } }");
        let value: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap();

        let x_elem = value.iter().find(|e| e["name"] == "x").unwrap();
        assert_eq!(x_elem["@type"], "Feature");
        assert!(x_elem.get("ownedTyping").is_some());
        assert!(x_elem.get("ownedMultiplicity").is_some());
    }

    #[test]
    fn serialize_feature_direction() {
        let json =
            compile_and_serialize("package P { type T { in feature f : T; out feature g : T; } }");
        let value: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap();

        let f_elem = value.iter().find(|e| e["name"] == "f").unwrap();
        assert_eq!(f_elem["direction"], "in");

        let g_elem = value.iter().find(|e| e["name"] == "g").unwrap();
        assert_eq!(g_elem["direction"], "out");
    }

    #[test]
    fn serialize_inherited_features_with_conjugation() {
        let json = compile_and_serialize(
            r#"package P {
                type A {
                    in feature f : A;
                    out feature g : A;
                }
                type B ~ A {}
            }"#,
        );
        let value: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap();

        let b_elem = value.iter().find(|e| e["name"] == "B").unwrap();
        let inherited = b_elem["inheritedFeature"].as_array();
        assert!(inherited.is_some(), "B should have inheritedFeature");
        let inherited = inherited.unwrap();
        assert_eq!(inherited.len(), 2);

        for inh in inherited {
            assert_eq!(inh["inheritanceKind"], "conjugation");
            assert!(inh.get("direction").is_some());
        }
    }

    #[test]
    fn output_is_valid_json() {
        let json = compile_and_serialize("package P { type A { feature f : A; } }");
        let parsed: Result<Vec<serde_json::Value>, _> = serde_json::from_str(&json);
        assert!(parsed.is_ok());
    }

    #[test]
    fn serialize_multiplicity_with_feature_ref() {
        let json = compile_and_serialize(
            "package P { type T { feature n : T; feature x : T [1..n]; } }",
        );
        let value: Vec<serde_json::Value> =
            serde_json::from_str(&json).unwrap();

        let x_elem =
            value.iter().find(|e| e["name"] == "x").unwrap();
        let mult = &x_elem["ownedMultiplicity"];
        assert_eq!(mult["@type"], "MultiplicityRange");
        assert_eq!(mult["lowerBound"], 1);
        assert_eq!(
            mult["upperBound"]["@type"],
            "FeatureReferenceExpression",
            "upper bound should serialize as FeatureReferenceExpression"
        );
    }
}
