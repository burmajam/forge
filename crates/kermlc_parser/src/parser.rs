use kermlc_ast::*;
use kermlc_diagnostics::{Diagnostic, DiagnosticSink, FileId, Label, Span};
use kermlc_intern::{Arena, StringInterner, SymbolId};
use kermlc_lexer::{Lexer, Token, TokenKind};

/// The result of parsing a source file.
pub struct ParseResult {
    pub source_file: SourceFile,
    pub packages: Arena<PackageDecl>,
    pub types: Arena<TypeDecl>,
    pub features: Arena<FeatureDecl>,
    pub conjugations: Arena<ConjugationDecl>,
}

pub struct Parser<'a> {
    source: &'a str,
    tokens: Vec<Token>,
    pos: usize,
    file: FileId,
    interner: &'a mut StringInterner,
    sink: &'a mut DiagnosticSink,
    packages: Arena<PackageDecl>,
    types: Arena<TypeDecl>,
    features: Arena<FeatureDecl>,
    conjugations: Arena<ConjugationDecl>,
}

impl<'a> Parser<'a> {
    pub fn parse(
        source: &'a str,
        file: FileId,
        interner: &'a mut StringInterner,
        sink: &'a mut DiagnosticSink,
    ) -> ParseResult {
        // Lex all tokens, filtering whitespace and comments
        let mut lexer = Lexer::new(source, file);
        let mut tokens = Vec::new();
        loop {
            let tok = lexer.next_token();
            match tok.kind {
                TokenKind::Whitespace | TokenKind::Comment => continue,
                TokenKind::Eof => {
                    tokens.push(tok);
                    break;
                }
                _ => tokens.push(tok),
            }
        }

        let mut parser = Parser {
            source,
            tokens,
            pos: 0,
            file,
            interner,
            sink,
            packages: Arena::new(),
            types: Arena::new(),
            features: Arena::new(),
            conjugations: Arena::new(),
        };

        let start = parser.current_span();
        let mut top_packages = Vec::new();
        let mut top_members = Vec::new();

        while !parser.at(TokenKind::Eof) {
            let before = parser.pos;
            match parser.peek() {
                TokenKind::Package => {
                    if let Some(id) = parser.parse_package() {
                        top_packages.push(id);
                    }
                }
                TokenKind::Type => {
                    if let Some(id) = parser.parse_type_decl() {
                        top_members.push(Member::Type(id));
                    }
                }
                TokenKind::Feature | TokenKind::In | TokenKind::Out | TokenKind::InOut => {
                    if let Some(id) = parser.parse_feature_decl() {
                        top_members.push(Member::Feature(id));
                    }
                }
                TokenKind::Conjugation => {
                    if let Some(id) = parser.parse_conjugation_decl() {
                        top_members.push(Member::Conjugation(id));
                    }
                }
                TokenKind::Import => {
                    // Top-level imports not attached to a package — skip with error
                    parser.error_at_current("import must be inside a package");
                    parser.bump();
                    parser.synchronize();
                }
                _ => {
                    parser.error_at_current(
                        "expected package, type, feature, or conjugation declaration",
                    );
                    parser.synchronize();
                }
            }
            // Safety: ensure we always advance to prevent infinite loops
            if parser.pos == before && !parser.at(TokenKind::Eof) {
                parser.bump();
            }
        }

        let end = parser.current_span();
        let span = if start.file == end.file {
            Span::new(start.file, start.start, end.end)
        } else {
            start
        };

        ParseResult {
            source_file: SourceFile {
                packages: top_packages,
                members: top_members,
                span,
            },
            packages: parser.packages,
            types: parser.types,
            features: parser.features,
            conjugations: parser.conjugations,
        }
    }

    // ── Token access ─────────────────────────────────────────────────────

    fn peek(&self) -> TokenKind {
        self.tokens.get(self.pos).map_or(TokenKind::Eof, |t| t.kind)
    }

    fn at(&self, kind: TokenKind) -> bool {
        self.peek() == kind
    }

    fn current_span(&self) -> Span {
        self.tokens
            .get(self.pos)
            .map_or(Span::new(self.file, 0, 0), |t| t.span)
    }

    fn bump(&mut self) -> Token {
        let tok = self.tokens[self.pos].clone();
        if self.pos < self.tokens.len() - 1 {
            self.pos += 1;
        }
        tok
    }

    fn expect(&mut self, kind: TokenKind) -> Option<Token> {
        if self.at(kind) {
            Some(self.bump())
        } else {
            self.error_at_current(&format!("expected {:?}", kind));
            None
        }
    }

    fn text(&self, span: Span) -> &str {
        &self.source[span.start as usize..span.end as usize]
    }

    fn intern_span(&mut self, span: Span) -> SymbolId {
        let text = &self.source[span.start as usize..span.end as usize];
        self.interner.intern(text)
    }

    // ── Error handling ───────────────────────────────────────────────────

    fn error_at_current(&mut self, msg: &str) {
        let span = self.current_span();
        self.sink
            .emit(Diagnostic::error(msg).with_label(Label::primary(span, msg)));
    }

    /// Skip tokens until we reach a synchronization point.
    fn synchronize(&mut self) {
        loop {
            match self.peek() {
                TokenKind::Eof => break,
                TokenKind::Package
                | TokenKind::Type
                | TokenKind::Feature
                | TokenKind::Import
                | TokenKind::Conjugation
                | TokenKind::In
                | TokenKind::Out
                | TokenKind::InOut => break,
                TokenKind::RBrace => {
                    self.bump();
                    break;
                }
                TokenKind::Semicolon => {
                    self.bump();
                    break;
                }
                _ => {
                    self.bump();
                }
            }
        }
    }

    // ── Parsing rules ────────────────────────────────────────────────────

    fn parse_package(&mut self) -> Option<PackageId> {
        let start = self.current_span();
        self.expect(TokenKind::Package)?;

        let name_tok = self.expect(TokenKind::Ident)?;
        let name = self.intern_span(name_tok.span);

        let mut imports = Vec::new();
        let mut members = Vec::new();

        if self.expect(TokenKind::LBrace).is_none() {
            // Try to recover
            return Some(self.packages.alloc(PackageDecl {
                name,
                span: Span::new(start.file, start.start, self.current_span().end),
                imports,
                members,
            }));
        }

        while !self.at(TokenKind::RBrace) && !self.at(TokenKind::Eof) {
            let before = self.pos;
            match self.peek() {
                TokenKind::Import => {
                    if let Some(import) = self.parse_import() {
                        imports.push(import);
                    }
                }
                TokenKind::Package => {
                    if let Some(id) = self.parse_package() {
                        members.push(Member::Package(id));
                    }
                }
                TokenKind::Type => {
                    if let Some(id) = self.parse_type_decl() {
                        members.push(Member::Type(id));
                    }
                }
                TokenKind::Feature | TokenKind::In | TokenKind::Out | TokenKind::InOut => {
                    if let Some(id) = self.parse_feature_decl() {
                        members.push(Member::Feature(id));
                    }
                }
                TokenKind::Conjugation => {
                    if let Some(id) = self.parse_conjugation_decl() {
                        members.push(Member::Conjugation(id));
                    }
                }
                _ => {
                    self.error_at_current(
                        "expected import, package, type, feature, \
                         or conjugation declaration",
                    );
                    self.synchronize();
                }
            }
            // Safety: ensure we always advance to prevent infinite loops
            if self.pos == before && !self.at(TokenKind::Eof) {
                self.bump();
            }
        }

        let end_span = self.current_span();
        if self.at(TokenKind::RBrace) {
            self.bump();
        } else {
            self.error_at_current("expected `}`");
        }

        let span = Span::new(start.file, start.start, end_span.end);
        Some(self.packages.alloc(PackageDecl {
            name,
            span,
            imports,
            members,
        }))
    }

    fn parse_import(&mut self) -> Option<ImportDecl> {
        let start = self.current_span();
        self.expect(TokenKind::Import)?;

        let path = self.parse_qualified_name()?;
        let mut is_wildcard = false;

        // Check for ::*
        if self.at(TokenKind::ColonColon) {
            let next_pos = self.pos + 1;
            if next_pos < self.tokens.len() && self.tokens[next_pos].kind == TokenKind::Star {
                self.bump(); // ::
                self.bump(); // *
                is_wildcard = true;
            }
        } else if self.at(TokenKind::Star) {
            // The qualified name might have ended at ::, then we see *
            self.bump();
            is_wildcard = true;
        }

        let end = self.current_span();
        self.expect(TokenKind::Semicolon);

        Some(ImportDecl {
            path,
            is_wildcard,
            span: Span::new(start.file, start.start, end.end),
        })
    }

    fn parse_type_decl(&mut self) -> Option<TypeDeclId> {
        let start = self.current_span();
        self.expect(TokenKind::Type)?;

        let name_tok = self.expect(TokenKind::Ident)?;
        let name = self.intern_span(name_tok.span);

        let mut specializations = Vec::new();
        let mut conjugation = None;

        // Parse specialization: :> or specializes
        loop {
            if self.at(TokenKind::ColonGt) || self.at(TokenKind::Specializes) {
                self.bump();
                if let Some(qn) = self.parse_qualified_name() {
                    specializations.push(qn);
                }
                // Allow comma-separated specializations
                while self.at(TokenKind::Comma) {
                    self.bump();
                    if let Some(qn) = self.parse_qualified_name() {
                        specializations.push(qn);
                    }
                }
            } else {
                break;
            }
        }

        // Parse conjugation: ~ or conjugates
        if self.at(TokenKind::Tilde) || self.at(TokenKind::Conjugates) {
            self.bump();
            conjugation = self.parse_qualified_name();
        }

        let mut members = Vec::new();

        // Body: { ... } or ;
        if self.at(TokenKind::LBrace) {
            self.bump();
            while !self.at(TokenKind::RBrace) && !self.at(TokenKind::Eof) {
                let before = self.pos;
                match self.peek() {
                    TokenKind::Type => {
                        if let Some(id) = self.parse_type_decl() {
                            members.push(Member::Type(id));
                        }
                    }
                    TokenKind::Feature | TokenKind::In | TokenKind::Out | TokenKind::InOut => {
                        if let Some(id) = self.parse_feature_decl() {
                            members.push(Member::Feature(id));
                        }
                    }
                    TokenKind::Conjugation => {
                        if let Some(id) = self.parse_conjugation_decl() {
                            members.push(Member::Conjugation(id));
                        }
                    }
                    _ => {
                        self.error_at_current("expected type, feature, or conjugation declaration");
                        self.synchronize();
                    }
                }
                // Safety: ensure we always advance to prevent infinite loops
                if self.pos == before && !self.at(TokenKind::Eof) {
                    self.bump();
                }
            }
            if self.at(TokenKind::RBrace) {
                self.bump();
            } else {
                self.error_at_current("expected `}`");
            }
        } else if self.at(TokenKind::Semicolon) {
            self.bump();
        }

        let end = self.current_span();
        let span = Span::new(start.file, start.start, end.start);

        Some(self.types.alloc(TypeDecl {
            name,
            span,
            specializations,
            conjugation,
            members,
        }))
    }

    fn parse_conjugation_decl(&mut self) -> Option<ConjugationDeclId> {
        let start = self.current_span();
        self.expect(TokenKind::Conjugation)?;

        let name_tok = self.expect(TokenKind::Ident)?;
        let name = self.intern_span(name_tok.span);

        if self.expect(TokenKind::Conjugate).is_none() {
            self.synchronize();
            return None;
        }

        let conjugated_type = match self.parse_qualified_name() {
            Some(qn) => qn,
            None => {
                self.synchronize();
                return None;
            }
        };

        if self.expect(TokenKind::Conjugates).is_none() {
            self.synchronize();
            return None;
        }

        let original_type = match self.parse_qualified_name() {
            Some(qn) => qn,
            None => {
                self.synchronize();
                return None;
            }
        };

        let end = self.current_span();
        self.expect(TokenKind::Semicolon);

        let span = Span::new(start.file, start.start, end.end);
        Some(self.conjugations.alloc(ConjugationDecl {
            name,
            span,
            conjugated_type,
            original_type,
        }))
    }

    fn parse_feature_decl(&mut self) -> Option<FeatureDeclId> {
        let start = self.current_span();

        // Parse optional direction modifier
        let direction = match self.peek() {
            TokenKind::In => {
                self.bump();
                Some(FeatureDirection::In)
            }
            TokenKind::Out => {
                self.bump();
                Some(FeatureDirection::Out)
            }
            TokenKind::InOut => {
                self.bump();
                Some(FeatureDirection::InOut)
            }
            _ => None,
        };

        self.expect(TokenKind::Feature)?;

        let name_tok = self.expect(TokenKind::Ident)?;
        let name = self.intern_span(name_tok.span);

        let mut type_ref = None;
        let mut conjugation = None;
        let mut chain = None;
        let mut multiplicity = None;

        // Parse typing `:` or conjugation `~`/`conjugates`
        if self.at(TokenKind::Colon) {
            self.bump();
            if self.at(TokenKind::Tilde) || self.at(TokenKind::Conjugates) {
                let conj_start = self.current_span();
                self.bump();
                if let Some(qn) = self.parse_qualified_name() {
                    let span = Span::new(conj_start.file, conj_start.start, qn.span.end);
                    type_ref = Some(TypeExpr::Conjugated(qn, span));
                }
            } else if let Some(qn) = self.parse_qualified_name() {
                type_ref = Some(TypeExpr::Named(qn));
            }
        } else if self.at(TokenKind::Tilde) || self.at(TokenKind::Conjugates) {
            self.bump();
            conjugation = self.parse_qualified_name();
        }

        // Parse chaining: chains a.b.c
        if self.at(TokenKind::Chains) {
            self.bump();
            chain = self.parse_feature_chain();
        }

        // Parse multiplicity: [...]
        if self.at(TokenKind::LBracket) {
            multiplicity = self.parse_multiplicity();
        }

        let end = self.current_span();
        // Consume trailing semicolon
        if self.at(TokenKind::Semicolon) {
            self.bump();
        }

        let span = Span::new(start.file, start.start, end.end);

        Some(self.features.alloc(FeatureDecl {
            name,
            span,
            direction,
            type_ref,
            conjugation,
            chain,
            multiplicity,
        }))
    }

    fn parse_qualified_name(&mut self) -> Option<QualifiedName> {
        let start = self.current_span();
        let name_tok = self.expect(TokenKind::Ident)?;
        let mut segments = vec![self.intern_span(name_tok.span)];

        while self.at(TokenKind::ColonColon) {
            // Check if next is an ident (not *)
            let next_pos = self.pos + 1;
            if next_pos < self.tokens.len() && self.tokens[next_pos].kind == TokenKind::Ident {
                self.bump(); // ::
                let seg_tok = self.bump();
                segments.push(self.intern_span(seg_tok.span));
            } else {
                break;
            }
        }

        let end = self
            .tokens
            .get(self.pos.saturating_sub(1))
            .map_or(start, |t| t.span);
        Some(QualifiedName {
            segments,
            span: Span::new(start.file, start.start, end.end),
        })
    }

    fn parse_feature_chain(&mut self) -> Option<FeatureChain> {
        let start = self.current_span();
        let first = self.parse_qualified_name()?;
        let mut segments = vec![first];

        while self.at(TokenKind::Dot) {
            self.bump();
            if let Some(qn) = self.parse_qualified_name() {
                segments.push(qn);
            } else {
                break;
            }
        }

        let end = self
            .tokens
            .get(self.pos.saturating_sub(1))
            .map_or(start, |t| t.span);

        Some(FeatureChain {
            segments,
            span: Span::new(start.file, start.start, end.end),
        })
    }

    fn parse_multiplicity(&mut self) -> Option<Multiplicity> {
        let start = self.current_span();
        self.expect(TokenKind::LBracket)?;

        let first = self.parse_expr_atom()?;

        let (lower, upper) = if self.at(TokenKind::DotDot) {
            self.bump();
            (Some(first), self.parse_expr_atom())
        } else {
            (None, Some(first))
        };

        let end = self.current_span();
        self.expect(TokenKind::RBracket);

        Some(Multiplicity {
            lower,
            upper,
            span: Span::new(start.file, start.start, end.end),
        })
    }

    fn parse_expr_atom(&mut self) -> Option<Expr> {
        match self.peek() {
            TokenKind::IntLiteral => {
                let tok = self.bump();
                let text = self.text(tok.span);
                let value = match text.parse::<u64>() {
                    Ok(v) => v,
                    Err(_) => {
                        self.sink.emit(
                            Diagnostic::error("integer literal out of range")
                                .with_label(Label::primary(tok.span, "exceeds u64::MAX")),
                        );
                        0
                    }
                };
                Some(Expr::IntLiteral {
                    value,
                    span: tok.span,
                })
            }
            TokenKind::Star => {
                let tok = self.bump();
                Some(Expr::Star { span: tok.span })
            }
            TokenKind::Ident => {
                let qn = self.parse_qualified_name()?;
                Some(Expr::Name { name: qn })
            }
            _ => {
                self.error_at_current("expected expression");
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kermlc_diagnostics::{DiagnosticSink, SourceMap};
    use kermlc_intern::StringInterner;

    fn parse(input: &str) -> (ParseResult, StringInterner, DiagnosticSink) {
        let mut interner = StringInterner::new();
        let mut source_map = SourceMap::new();
        let mut sink = DiagnosticSink::new();
        let file_id = source_map.add_file("test.kerml".into(), input.into());
        let result = Parser::parse(input, file_id, &mut interner, &mut sink);
        (result, interner, sink)
    }

    #[test]
    fn parse_empty_package() {
        let (result, interner, sink) = parse("package Foo {}");
        assert!(!sink.has_errors());
        assert_eq!(result.source_file.packages.len(), 1);
        let pkg = &result.packages[result.source_file.packages[0]];
        assert_eq!(interner.resolve(pkg.name), "Foo");
    }

    #[test]
    fn parse_type_with_specialization() {
        let (result, _interner, sink) = parse("package Vehicles { type Car :> Vehicle {} }");
        assert!(!sink.has_errors());
        let pkg = &result.packages[result.source_file.packages[0]];
        assert_eq!(pkg.members.len(), 1);
    }

    #[test]
    fn parse_feature_decl() {
        let (_result, _interner, sink) =
            parse("package P { type T { feature x : Integer [0..1]; } }");
        assert!(!sink.has_errors());
    }

    #[test]
    fn parse_import() {
        let (result, _, sink) = parse("package P { import Vehicles::*; }");
        assert!(!sink.has_errors());
        let pkg = &result.packages[result.source_file.packages[0]];
        assert_eq!(pkg.imports.len(), 1);
        assert!(pkg.imports[0].is_wildcard);
    }

    #[test]
    fn parse_in_feature() {
        let (result, _interner, sink) = parse("package P { type T { in feature f : Tp; } }");
        assert!(!sink.has_errors());
        let pkg = &result.packages[result.source_file.packages[0]];
        let Member::Type(ty_id) = &pkg.members[0] else {
            panic!("expected type member");
        };
        let ty = &result.types[*ty_id];
        let Member::Feature(feat_id) = &ty.members[0] else {
            panic!("expected feature member");
        };
        let feat = &result.features[*feat_id];
        assert_eq!(feat.direction, Some(FeatureDirection::In));
    }

    #[test]
    fn parse_out_feature() {
        let (result, _interner, sink) = parse("package P { type T { out feature g : Tp; } }");
        assert!(!sink.has_errors());
        let pkg = &result.packages[result.source_file.packages[0]];
        let Member::Type(ty_id) = &pkg.members[0] else {
            panic!("expected type member");
        };
        let ty = &result.types[*ty_id];
        let Member::Feature(feat_id) = &ty.members[0] else {
            panic!("expected feature member");
        };
        let feat = &result.features[*feat_id];
        assert_eq!(feat.direction, Some(FeatureDirection::Out));
    }

    #[test]
    fn parse_inout_feature() {
        let (result, _interner, sink) = parse("package P { type T { inout feature h : Tp; } }");
        assert!(!sink.has_errors());
        let pkg = &result.packages[result.source_file.packages[0]];
        let Member::Type(ty_id) = &pkg.members[0] else {
            panic!("expected type member");
        };
        let ty = &result.types[*ty_id];
        let Member::Feature(feat_id) = &ty.members[0] else {
            panic!("expected feature member");
        };
        let feat = &result.features[*feat_id];
        assert_eq!(feat.direction, Some(FeatureDirection::InOut));
    }

    #[test]
    fn parse_undirected_feature() {
        let (result, _interner, sink) = parse("package P { type T { feature x : Tp; } }");
        assert!(!sink.has_errors());
        let pkg = &result.packages[result.source_file.packages[0]];
        let Member::Type(ty_id) = &pkg.members[0] else {
            panic!("expected type member");
        };
        let ty = &result.types[*ty_id];
        let Member::Feature(feat_id) = &ty.members[0] else {
            panic!("expected feature member");
        };
        let feat = &result.features[*feat_id];
        assert_eq!(feat.direction, None);
    }

    #[test]
    fn error_recovery_missing_brace() {
        let (result, _, sink) = parse("package Foo { type Bar {} ");
        // Should have an error for missing closing brace
        assert!(sink.has_errors());
        // But should still have parsed some content
        assert!(!result.source_file.packages.is_empty());
    }

    #[test]
    fn parse_feature_conjugation_tilde() {
        let (result, interner, sink) = parse("package P { type T { feature g ~ T; } }");
        assert!(!sink.has_errors(), "errors: {:?}", sink.diagnostics());
        let pkg = &result.packages[result.source_file.packages[0]];
        let Member::Type(ty_id) = &pkg.members[0] else {
            panic!("expected type member");
        };
        let ty = &result.types[*ty_id];
        let Member::Feature(feat_id) = &ty.members[0] else {
            panic!("expected feature member");
        };
        let feat = &result.features[*feat_id];
        assert_eq!(interner.resolve(feat.name), "g");
        assert!(feat.conjugation.is_some(), "should have conjugation");
        assert!(feat.type_ref.is_none(), "should NOT have type_ref");
    }

    #[test]
    fn parse_feature_conjugation_keyword() {
        let (result, _interner, sink) = parse("package P { type T { feature g conjugates T; } }");
        assert!(!sink.has_errors(), "errors: {:?}", sink.diagnostics());
        let pkg = &result.packages[result.source_file.packages[0]];
        let Member::Type(ty_id) = &pkg.members[0] else {
            panic!("expected type member");
        };
        let ty = &result.types[*ty_id];
        let Member::Feature(feat_id) = &ty.members[0] else {
            panic!("expected feature member");
        };
        let feat = &result.features[*feat_id];
        assert!(feat.conjugation.is_some());
    }

    #[test]
    fn parse_feature_conjugation_qualified() {
        let (result, _interner, sink) = parse("package P { type T { feature g ~ A::f; } }");
        assert!(!sink.has_errors(), "errors: {:?}", sink.diagnostics());
        let pkg = &result.packages[result.source_file.packages[0]];
        let Member::Type(ty_id) = &pkg.members[0] else {
            panic!("expected type member");
        };
        let ty = &result.types[*ty_id];
        let Member::Feature(feat_id) = &ty.members[0] else {
            panic!("expected feature member");
        };
        let feat = &result.features[*feat_id];
        let conj = feat.conjugation.as_ref().expect("should have conjugation");
        assert_eq!(conj.segments.len(), 2, "A::f has 2 segments");
    }

    #[test]
    fn parse_inline_conjugated_type_ref() {
        let (result, interner, sink) = parse("package P { type T { feature f : ~T; } }");
        assert!(!sink.has_errors(), "errors: {:?}", sink.diagnostics());
        let pkg = &result.packages[result.source_file.packages[0]];
        let Member::Type(ty_id) = &pkg.members[0] else {
            panic!("expected type member");
        };
        let ty = &result.types[*ty_id];
        let Member::Feature(feat_id) = &ty.members[0] else {
            panic!("expected feature member");
        };
        let feat = &result.features[*feat_id];
        assert_eq!(interner.resolve(feat.name), "f");
        let type_ref = feat.type_ref.as_ref().expect("should have type_ref");
        match type_ref {
            TypeExpr::Conjugated(qn, _) => {
                assert_eq!(qn.segments.len(), 1);
                assert_eq!(interner.resolve(qn.segments[0]), "T");
            }
            _ => panic!("expected TypeExpr::Conjugated, got {:?}", type_ref),
        }
        assert!(
            feat.conjugation.is_none(),
            "conjugation field should be None"
        );
    }

    #[test]
    fn parse_inline_conjugated_type_ref_qualified() {
        let (result, interner, sink) = parse("package P { type T { feature f : ~A::B; } }");
        assert!(!sink.has_errors(), "errors: {:?}", sink.diagnostics());
        let pkg = &result.packages[result.source_file.packages[0]];
        let Member::Type(ty_id) = &pkg.members[0] else {
            panic!("expected type member");
        };
        let ty = &result.types[*ty_id];
        let Member::Feature(feat_id) = &ty.members[0] else {
            panic!("expected feature member");
        };
        let feat = &result.features[*feat_id];
        match feat.type_ref.as_ref().unwrap() {
            TypeExpr::Conjugated(qn, _) => {
                assert_eq!(qn.segments.len(), 2);
                assert_eq!(interner.resolve(qn.segments[0]), "A");
                assert_eq!(interner.resolve(qn.segments[1]), "B");
            }
            _ => panic!("expected TypeExpr::Conjugated"),
        }
    }

    #[test]
    fn parse_multiplicity_name_exact() {
        let (result, _interner, sink) = parse("package P { type T { feature x : T [n]; } }");
        assert!(!sink.has_errors(), "errors: {:?}", sink.diagnostics());
        let pkg = &result.packages[result.source_file.packages[0]];
        let feat_id = match &pkg.members[0] {
            Member::Type(id) => match &result.types[*id].members[0] {
                Member::Feature(fid) => *fid,
                _ => panic!("expected feature"),
            },
            _ => panic!("expected type"),
        };
        let feat = &result.features[feat_id];
        let mult = feat
            .multiplicity
            .as_ref()
            .expect("should have multiplicity");
        assert!(mult.lower.is_none(), "exact mult should have no lower");
        assert!(matches!(mult.upper, Some(Expr::Name { .. })));
    }

    #[test]
    fn parse_multiplicity_name_range() {
        let (_result, _interner, sink) = parse("package P { type T { feature x : T [a..b]; } }");
        assert!(!sink.has_errors(), "errors: {:?}", sink.diagnostics());
    }

    #[test]
    fn parse_multiplicity_int_to_name() {
        let (_result, _interner, sink) = parse("package P { type T { feature x : T [1..n]; } }");
        assert!(!sink.has_errors(), "errors: {:?}", sink.diagnostics());
    }

    #[test]
    fn parse_multiplicity_name_to_star() {
        let (_result, _interner, sink) = parse("package P { type T { feature x : T [n..*]; } }");
        assert!(!sink.has_errors(), "errors: {:?}", sink.diagnostics());
    }

    #[test]
    fn parse_multiplicity_qualified_name() {
        let (_result, _interner, sink) =
            parse("package P { type T { feature x : T [Pkg::count]; } }");
        assert!(!sink.has_errors(), "errors: {:?}", sink.diagnostics());
    }
}
