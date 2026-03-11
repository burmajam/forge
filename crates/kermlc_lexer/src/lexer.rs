use crate::token::{Token, TokenKind};
use kermlc_diagnostics::{FileId, Span};

pub struct Lexer<'a> {
    source: &'a str,
    bytes: &'a [u8],
    pos: u32,
    file: FileId,
}

impl<'a> Lexer<'a> {
    pub fn new(source: &'a str, file: FileId) -> Self {
        Self {
            source,
            bytes: source.as_bytes(),
            pos: 0,
            file,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos as usize).copied()
    }

    fn peek_next(&self) -> Option<u8> {
        self.bytes.get((self.pos + 1) as usize).copied()
    }

    fn advance(&mut self) -> u8 {
        let b = self.bytes[self.pos as usize];
        self.pos += 1;
        b
    }

    fn span_from(&self, start: u32) -> Span {
        Span::new(self.file, start, self.pos)
    }

    fn eat_while(&mut self, pred: impl Fn(u8) -> bool) {
        while self.pos < self.bytes.len() as u32 && pred(self.bytes[self.pos as usize]) {
            self.pos += 1;
        }
    }

    pub fn next_token(&mut self) -> Token {
        if self.pos >= self.bytes.len() as u32 {
            return Token {
                kind: TokenKind::Eof,
                span: Span::new(self.file, self.pos, self.pos),
            };
        }

        let start = self.pos;
        let b = self.advance();

        let kind = match b {
            // Whitespace
            b' ' | b'\t' | b'\r' | b'\n' => {
                self.eat_while(|c| matches!(c, b' ' | b'\t' | b'\r' | b'\n'));
                TokenKind::Whitespace
            }

            // Comments or Slash
            b'/' => {
                if self.peek() == Some(b'/') {
                    // Line comment
                    self.eat_while(|c| c != b'\n');
                    TokenKind::Comment
                } else if self.peek() == Some(b'*') {
                    // Block comment (nestable)
                    self.pos += 1; // skip *
                    let mut depth = 1u32;
                    while self.pos < self.bytes.len() as u32 && depth > 0 {
                        if self.peek() == Some(b'*') && self.peek_next() == Some(b'/') {
                            self.pos += 2;
                            depth -= 1;
                        } else if self.peek() == Some(b'/') && self.peek_next() == Some(b'*') {
                            self.pos += 2;
                            depth += 1;
                        } else {
                            self.pos += 1;
                        }
                    }
                    TokenKind::Comment
                } else {
                    TokenKind::Slash
                }
            }

            // Punctuation
            b'{' => TokenKind::LBrace,
            b'}' => TokenKind::RBrace,
            b'[' => TokenKind::LBracket,
            b']' => TokenKind::RBracket,
            b'(' => TokenKind::LParen,
            b')' => TokenKind::RParen,
            b';' => TokenKind::Semicolon,
            b',' => TokenKind::Comma,
            b'~' => TokenKind::Tilde,
            b'*' => TokenKind::Star,
            b'+' => TokenKind::Plus,
            b'-' => TokenKind::Minus,
            b'=' => TokenKind::Eq,

            // Colon variants: : :: :>
            b':' => {
                if self.peek() == Some(b':') {
                    self.pos += 1;
                    TokenKind::ColonColon
                } else if self.peek() == Some(b'>') {
                    self.pos += 1;
                    TokenKind::ColonGt
                } else {
                    TokenKind::Colon
                }
            }

            // Dot variants: . ..
            b'.' => {
                if self.peek() == Some(b'.') {
                    self.pos += 1;
                    TokenKind::DotDot
                } else {
                    TokenKind::Dot
                }
            }

            // Integer literals
            b'0'..=b'9' => {
                self.eat_while(|c| c.is_ascii_digit());
                TokenKind::IntLiteral
            }

            // Identifiers and keywords
            b'a'..=b'z' | b'A'..=b'Z' | b'_' => {
                self.eat_while(|c| c.is_ascii_alphanumeric() || c == b'_');
                let text = &self.source[start as usize..self.pos as usize];
                match text {
                    "package" => TokenKind::Package,
                    "import" => TokenKind::Import,
                    "type" => TokenKind::Type,
                    "feature" => TokenKind::Feature,
                    "specializes" => TokenKind::Specializes,
                    "conjugation" => TokenKind::Conjugation,
                    "conjugate" => TokenKind::Conjugate,
                    "conjugates" => TokenKind::Conjugates,
                    "chains" => TokenKind::Chains,
                    "in" => TokenKind::In,
                    "out" => TokenKind::Out,
                    "inout" => TokenKind::InOut,
                    _ => TokenKind::Ident,
                }
            }

            // String literals
            b'"' => {
                self.eat_while(|c| c != b'"');
                if self.peek() == Some(b'"') {
                    self.pos += 1; // consume closing quote
                }
                TokenKind::StringLiteral
            }

            _ => TokenKind::Error,
        };

        Token {
            kind,
            span: self.span_from(start),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::token::TokenKind;
    use kermlc_diagnostics::FileId;

    fn lex(input: &str) -> Vec<(TokenKind, &str)> {
        let mut lexer = Lexer::new(input, FileId(0));
        let mut tokens = Vec::new();
        loop {
            let tok = lexer.next_token();
            if tok.kind == TokenKind::Eof {
                break;
            }
            if tok.kind == TokenKind::Whitespace || tok.kind == TokenKind::Comment {
                continue;
            }
            let text = &input[tok.span.start as usize..tok.span.end as usize];
            tokens.push((tok.kind, text));
        }
        tokens
    }

    #[test]
    fn lex_package_decl() {
        let tokens = lex("package Foo {}");
        assert_eq!(
            tokens,
            vec![
                (TokenKind::Package, "package"),
                (TokenKind::Ident, "Foo"),
                (TokenKind::LBrace, "{"),
                (TokenKind::RBrace, "}"),
            ]
        );
    }

    #[test]
    fn lex_type_with_specialization() {
        let tokens = lex("type Car :> Vehicle {}");
        assert_eq!(
            tokens,
            vec![
                (TokenKind::Type, "type"),
                (TokenKind::Ident, "Car"),
                (TokenKind::ColonGt, ":>"),
                (TokenKind::Ident, "Vehicle"),
                (TokenKind::LBrace, "{"),
                (TokenKind::RBrace, "}"),
            ]
        );
    }

    #[test]
    fn lex_feature_with_multiplicity() {
        let tokens = lex("feature wheels : Wheel [4];");
        assert_eq!(
            tokens,
            vec![
                (TokenKind::Feature, "feature"),
                (TokenKind::Ident, "wheels"),
                (TokenKind::Colon, ":"),
                (TokenKind::Ident, "Wheel"),
                (TokenKind::LBracket, "["),
                (TokenKind::IntLiteral, "4"),
                (TokenKind::RBracket, "]"),
                (TokenKind::Semicolon, ";"),
            ]
        );
    }

    #[test]
    fn lex_qualified_name() {
        let tokens = lex("Vehicles::Car");
        assert_eq!(
            tokens,
            vec![
                (TokenKind::Ident, "Vehicles"),
                (TokenKind::ColonColon, "::"),
                (TokenKind::Ident, "Car"),
            ]
        );
    }

    #[test]
    fn lex_conjugation() {
        let tokens = lex("type T ~ U {}");
        assert_eq!(
            tokens,
            vec![
                (TokenKind::Type, "type"),
                (TokenKind::Ident, "T"),
                (TokenKind::Tilde, "~"),
                (TokenKind::Ident, "U"),
                (TokenKind::LBrace, "{"),
                (TokenKind::RBrace, "}"),
            ]
        );
    }

    #[test]
    fn lex_comments() {
        let input = "package /* block */ Foo // line\n{}";
        let mut lexer = Lexer::new(input, FileId(0));
        let mut kinds = Vec::new();
        loop {
            let tok = lexer.next_token();
            if tok.kind == TokenKind::Eof {
                break;
            }
            if tok.kind != TokenKind::Whitespace {
                kinds.push(tok.kind);
            }
        }
        assert!(kinds.contains(&TokenKind::Comment));
    }

    #[test]
    fn lex_direction_keywords() {
        let tokens = lex("in out inout");
        assert_eq!(
            tokens,
            vec![
                (TokenKind::In, "in"),
                (TokenKind::Out, "out"),
                (TokenKind::InOut, "inout"),
            ]
        );
    }

    #[test]
    fn lex_dot_dot() {
        let tokens = lex("[0..1]");
        assert_eq!(
            tokens,
            vec![
                (TokenKind::LBracket, "["),
                (TokenKind::IntLiteral, "0"),
                (TokenKind::DotDot, ".."),
                (TokenKind::IntLiteral, "1"),
                (TokenKind::RBracket, "]"),
            ]
        );
    }
}
