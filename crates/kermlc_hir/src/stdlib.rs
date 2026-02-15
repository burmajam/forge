use crate::types::*;
use kermlc_diagnostics::Span;
use kermlc_intern::StringInterner;

/// Load the minimal Kernel Semantic Library base types into the model.
///
/// Creates hardcoded Def entries for:
/// - Anything (root of all types)
/// - Object (structures)
/// - DataValue (data types)
/// - Occurrence (occurrences)
/// - Performance (behaviors)
/// - Link (associations)
///
/// All types implicitly specialize Anything (except Anything itself).
pub fn load_stdlib(model: &mut SemanticModel, interner: &mut StringInterner) -> StdlibDefs {
    let anything_id = model.alloc_def(Def::new(
        interner.intern("Anything"),
        DefKind::Type,
        Span::dummy(),
    ));

    let object_id = {
        let mut def = Def::new(interner.intern("Object"), DefKind::Type, Span::dummy());
        def.specializations.push(NameRef {
            segments: vec![interner.intern("Anything")],
            span: Span::dummy(),
            resolution: ResolutionState::Resolved(anything_id),
        });
        model.alloc_def(def)
    };

    let data_value_id = {
        let mut def = Def::new(interner.intern("DataValue"), DefKind::Type, Span::dummy());
        def.specializations.push(NameRef {
            segments: vec![interner.intern("Anything")],
            span: Span::dummy(),
            resolution: ResolutionState::Resolved(anything_id),
        });
        model.alloc_def(def)
    };

    let occurrence_id = {
        let mut def = Def::new(interner.intern("Occurrence"), DefKind::Type, Span::dummy());
        def.specializations.push(NameRef {
            segments: vec![interner.intern("Anything")],
            span: Span::dummy(),
            resolution: ResolutionState::Resolved(anything_id),
        });
        model.alloc_def(def)
    };

    let performance_id = {
        let mut def = Def::new(interner.intern("Performance"), DefKind::Type, Span::dummy());
        def.specializations.push(NameRef {
            segments: vec![interner.intern("Anything")],
            span: Span::dummy(),
            resolution: ResolutionState::Resolved(anything_id),
        });
        model.alloc_def(def)
    };

    let link_id = {
        let mut def = Def::new(interner.intern("Link"), DefKind::Type, Span::dummy());
        def.specializations.push(NameRef {
            segments: vec![interner.intern("Anything")],
            span: Span::dummy(),
            resolution: ResolutionState::Resolved(anything_id),
        });
        model.alloc_def(def)
    };

    // Add stdlib types as roots
    model.roots.push(anything_id);
    model.roots.push(object_id);
    model.roots.push(data_value_id);
    model.roots.push(occurrence_id);
    model.roots.push(performance_id);
    model.roots.push(link_id);

    // Mark all stdlib types as type-checked (they are pre-resolved)
    model.defs[anything_id].type_checked = true;
    model.defs[object_id].type_checked = true;
    model.defs[data_value_id].type_checked = true;
    model.defs[occurrence_id].type_checked = true;
    model.defs[performance_id].type_checked = true;
    model.defs[link_id].type_checked = true;

    StdlibDefs {
        anything: anything_id,
        object: object_id,
        data_value: data_value_id,
        occurrence: occurrence_id,
        performance: performance_id,
        link: link_id,
    }
}

/// Holds DefIds for the standard library types for easy reference.
#[derive(Clone, Debug)]
pub struct StdlibDefs {
    pub anything: DefId,
    pub object: DefId,
    pub data_value: DefId,
    pub occurrence: DefId,
    pub performance: DefId,
    pub link: DefId,
}

#[cfg(test)]
mod tests {
    use super::*;
    use kermlc_intern::StringInterner;

    #[test]
    fn stdlib_creates_six_types() {
        let mut model = SemanticModel::new();
        let mut interner = StringInterner::new();
        let stdlib = load_stdlib(&mut model, &mut interner);

        assert_eq!(model.roots.len(), 6);
        assert_eq!(model.defs[stdlib.anything].kind, DefKind::Type);
        assert_eq!(interner.resolve(model.defs[stdlib.anything].name), "Anything");

        // Object specializes Anything
        let object = &model.defs[stdlib.object];
        assert_eq!(object.specializations.len(), 1);
        assert_eq!(
            object.specializations[0].resolution,
            ResolutionState::Resolved(stdlib.anything)
        );
    }
}
