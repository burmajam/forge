use kermlc_diagnostics::Span;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenKind {
    // Keywords
    Package,
    Import,
    Type,
    Feature,
    Specializes,
    Conjugates,
    Chains,

    // Literals
    IntLiteral,
    StringLiteral,

    // Identifiers
    Ident,

    // Punctuation
    LBrace,    // {
    RBrace,    // }
    LBracket,  // [
    RBracket,  // ]
    LParen,    // (
    RParen,    // )
    Semicolon, // ;
    Colon,     // :
    ColonColon, // ::
    ColonGt,   // :>
    Dot,       // .
    DotDot,    // ..
    Comma,     // ,
    Tilde,     // ~
    Star,      // *
    Plus,      // +
    Minus,     // -
    Slash,     // /
    Eq,        // =

    // Special
    Comment,
    Whitespace,
    Eof,
    Error,
}

#[derive(Clone, Debug)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}
