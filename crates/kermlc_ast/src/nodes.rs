use kermlc_diagnostics::Span;
use kermlc_intern::{Idx, SymbolId};

// Type aliases for readability
pub type PackageId = Idx<PackageDecl>;
pub type TypeDeclId = Idx<TypeDecl>;
pub type FeatureDeclId = Idx<FeatureDecl>;
pub type ConjugationDeclId = Idx<ConjugationDecl>;

/// A qualified name like `Vehicles::Car` or `Base::Anything`.
#[derive(Clone, Debug)]
pub struct QualifiedName {
    pub segments: Vec<SymbolId>,
    pub span: Span,
}

/// A member of a package or type body.
#[derive(Clone, Debug)]
pub enum Member {
    Package(PackageId),
    Type(TypeDeclId),
    Feature(FeatureDeclId),
    Conjugation(ConjugationDeclId),
}

/// `package Foo { ... }`
#[derive(Clone, Debug)]
pub struct PackageDecl {
    pub name: SymbolId,
    pub span: Span,
    pub imports: Vec<ImportDecl>,
    pub members: Vec<Member>,
}

/// `import Foo::Bar::*;`
#[derive(Clone, Debug)]
pub struct ImportDecl {
    pub path: QualifiedName,
    pub is_wildcard: bool,
    pub span: Span,
}

/// `type Car specializes Vehicle conjugates ~Truck { ... }`
#[derive(Clone, Debug)]
pub struct TypeDecl {
    pub name: SymbolId,
    pub span: Span,
    pub specializations: Vec<QualifiedName>,
    pub conjugation: Option<QualifiedName>,
    pub members: Vec<Member>,
}

/// Multiplicity like `[0..1]` or `[*]`
#[derive(Clone, Debug)]
pub struct Multiplicity {
    pub lower: Option<Expr>,
    pub upper: Option<Expr>,
    pub span: Span,
}

/// Feature chaining: `chains a.b.c`
#[derive(Clone, Debug)]
pub struct FeatureChain {
    pub segments: Vec<QualifiedName>,
    pub span: Span,
}

/// Direction modifier for a feature (in, out, inout).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeatureDirection {
    In,
    Out,
    InOut,
}

/// `feature wheels : Wheel [4];`
#[derive(Clone, Debug)]
pub struct FeatureDecl {
    pub name: SymbolId,
    pub span: Span,
    pub direction: Option<FeatureDirection>,
    pub type_ref: Option<QualifiedName>,
    pub conjugation: Option<QualifiedName>,
    pub chain: Option<FeatureChain>,
    pub multiplicity: Option<Multiplicity>,
}

/// `conjugation c1 conjugate Sink conjugates Source;`
#[derive(Clone, Debug)]
pub struct ConjugationDecl {
    pub name: SymbolId,
    pub span: Span,
    pub conjugated_type: QualifiedName,
    pub original_type: QualifiedName,
}

/// Expression node (minimal for milestone 1).
#[derive(Clone, Debug)]
pub enum Expr {
    IntLiteral {
        value: u64,
        span: Span,
    },
    Star {
        span: Span,
    }, // `*` for unbounded multiplicity
    Name {
        name: QualifiedName,
    },
    BinOp {
        op: BinOpKind,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
        span: Span,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOpKind {
    Add,
    Sub,
    Mul,
    Div,
    Range, // `..`
}

/// Root of a parsed file.
#[derive(Clone, Debug)]
pub struct SourceFile {
    pub packages: Vec<PackageId>,
    pub members: Vec<Member>, // top-level members outside packages
    pub span: Span,
}

#[cfg(test)]
mod tests {
    use super::*;
    use kermlc_intern::{Arena, StringInterner};

    #[test]
    fn build_simple_ast() {
        let mut interner = StringInterner::new();
        let mut packages = Arena::new();

        let pkg_name = interner.intern("Vehicles");
        let pkg_id = packages.alloc(PackageDecl {
            name: pkg_name,
            span: Span::dummy(),
            imports: vec![],
            members: vec![],
        });

        assert_eq!(interner.resolve(packages[pkg_id].name), "Vehicles");
    }

    #[test]
    fn type_decl_with_specialization() {
        let mut interner = StringInterner::new();
        let mut types = Arena::new();

        let type_id = types.alloc(TypeDecl {
            name: interner.intern("Car"),
            span: Span::dummy(),
            specializations: vec![QualifiedName {
                segments: vec![interner.intern("Vehicle")],
                span: Span::dummy(),
            }],
            conjugation: None,
            members: vec![],
        });

        assert_eq!(types[type_id].specializations.len(), 1);
    }
}
