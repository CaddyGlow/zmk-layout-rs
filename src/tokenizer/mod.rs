//! Tokenizer based on `logos` following `rust/PLAN.md` Phase 1.

use std::{fmt, ops::Range};

use logos::{Lexer, Logos};
use thiserror::Error;

/// Detailed source span with byte offsets and line/column coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenSpan {
    pub start: usize,
    pub end: usize,
    pub start_line: usize,
    pub start_column: usize,
    pub end_line: usize,
    pub end_column: usize,
}

impl TokenSpan {
    fn new(
        start: usize,
        end: usize,
        start_line: usize,
        start_column: usize,
        end_line: usize,
        end_column: usize,
    ) -> Self {
        Self {
            start,
            end,
            start_line,
            start_column,
            end_line,
            end_column,
        }
    }
}

/// Token produced by the tokenizer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub lexeme: String,
    pub span: TokenSpan,
}

/// Primary error type for tokenizer failures.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum LayoutError {
    #[error("{message} at {span}")]
    Parse { message: String, span: TokenSpan },
}

impl fmt::Display for TokenSpan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "bytes {}-{}, line {} column {}",
            self.start, self.end, self.start_line, self.start_column
        )
    }
}

/// Public token kinds emitted by the tokenizer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TokenKind {
    Identifier,
    Reference,
    Literal,
    LBrace,
    RBrace,
    AngleOpen,
    AngleClose,
    Comma,
    Semicolon,
    PreprocessorInclude,
    PreprocessorDefine,
    PreprocessorOther,
    LineComment,
    BlockComment,
    TemplateBlock,
    TemplateExpr,
    Whitespace,
}

/// Iterator over tokens for a given source input.
pub struct TokenStream<'source> {
    lexer: logos::Lexer<'source, RawToken>,
    source: &'source str,
    include_trivia: bool,
    tracker: PositionTracker<'source>,
    faulted: bool,
}

impl<'source> TokenStream<'source> {
    pub fn new(source: &'source str) -> Self {
        Self {
            lexer: RawToken::lexer(source),
            source,
            include_trivia: false,
            tracker: PositionTracker::new(source),
            faulted: false,
        }
    }

    /// Include whitespace trivia tokens when enabled.
    pub fn with_trivia(mut self, include: bool) -> Self {
        self.include_trivia = include;
        self
    }
}

impl<'source> Iterator for TokenStream<'source> {
    type Item = Result<Token, LayoutError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.faulted {
            return None;
        }
        while let Some(tok) = self.lexer.next() {
            let span_range = self.lexer.span();
            let span = self.tracker.record(span_range.clone());
            match tok {
                Ok(raw) => {
                    let kind = TokenKind::from(raw);
                    if !self.include_trivia && matches!(kind, TokenKind::Whitespace) {
                        continue;
                    }
                    let lexeme = self.lexer.slice().to_string();
                    return Some(Ok(Token { kind, lexeme, span }));
                }
                Err(()) => {
                    let err = self.build_error(span_range, span);
                    self.faulted = true;
                    return Some(Err(err));
                }
            }
        }
        None
    }
}

impl<'source> TokenStream<'source> {
    fn build_error(&self, range: Range<usize>, span: TokenSpan) -> LayoutError {
        let remainder = &self.source[range.start.min(self.source.len())..];
        let message = if remainder.starts_with("/*") {
            "unterminated block comment".to_string()
        } else if remainder.starts_with('"') {
            "unterminated string literal".to_string()
        } else if remainder.starts_with("{") {
            if remainder.starts_with("{{") {
                "unterminated template expression".to_string()
            } else {
                "unterminated template block".to_string()
            }
        } else {
            let bad_char = remainder.chars().next().unwrap_or('\0');
            format!("unexpected token `{}`", bad_char)
        };
        LayoutError::Parse { message, span }
    }
}

/// Convenience helper that collects all tokens for the source.
pub fn tokenize(source: &str) -> Result<Vec<Token>, LayoutError> {
    TokenStream::new(source).collect()
}

#[derive(Logos, Debug, Clone, Copy, PartialEq, Eq)]
#[logos(error = ())]
enum RawToken {
    #[regex(r"&[A-Za-z0-9_]+", priority = 200)]
    Reference,
    #[regex(r"[A-Za-z_][A-Za-z0-9_]*", priority = 100)]
    Identifier,
    #[regex(r#"\"([^\"\\]|\\.)*\""#, priority = 90)]
    StringLiteral,
    #[regex(r"[0-9]+", priority = 80)]
    NumberLiteral,
    #[token("{")]
    LBrace,
    #[token("}")]
    RBrace,
    #[token("<")]
    AngleOpen,
    #[token(">")]
    AngleClose,
    #[token(",")]
    Comma,
    #[token(";")]
    Semicolon,
    #[regex(r"//[^\n]*", priority = 70)]
    LineComment,
    #[token("/*", callback = block_comment, priority = 60)]
    BlockComment,
    #[regex(r"#include[^\n]*", priority = 50)]
    PreprocessorInclude,
    #[regex(r"#define[^\n]*", priority = 50)]
    PreprocessorDefine,
    #[regex(r"#(?:if|ifdef|ifndef|elif|else|endif|undef)[^\n]*", priority = 40)]
    PreprocessorOther,
    #[token("{%", callback = template_block, priority = 30)]
    TemplateBlock,
    #[token("{{", callback = template_expr, priority = 30)]
    TemplateExpr,
    #[regex(r"[ \t\r\n]+", priority = 10)]
    Whitespace,
}

impl From<RawToken> for TokenKind {
    fn from(value: RawToken) -> Self {
        match value {
            RawToken::Identifier => TokenKind::Identifier,
            RawToken::Reference => TokenKind::Reference,
            RawToken::StringLiteral | RawToken::NumberLiteral => TokenKind::Literal,
            RawToken::LBrace => TokenKind::LBrace,
            RawToken::RBrace => TokenKind::RBrace,
            RawToken::AngleOpen => TokenKind::AngleOpen,
            RawToken::AngleClose => TokenKind::AngleClose,
            RawToken::Comma => TokenKind::Comma,
            RawToken::Semicolon => TokenKind::Semicolon,
            RawToken::LineComment => TokenKind::LineComment,
            RawToken::BlockComment => TokenKind::BlockComment,
            RawToken::PreprocessorInclude => TokenKind::PreprocessorInclude,
            RawToken::PreprocessorDefine => TokenKind::PreprocessorDefine,
            RawToken::PreprocessorOther => TokenKind::PreprocessorOther,
            RawToken::TemplateBlock => TokenKind::TemplateBlock,
            RawToken::TemplateExpr => TokenKind::TemplateExpr,
            RawToken::Whitespace => TokenKind::Whitespace,
        }
    }
}

fn block_comment(lexer: &mut Lexer<'_, RawToken>) -> Option<()> {
    consume_until(lexer, "*/")
}

fn template_block(lexer: &mut Lexer<'_, RawToken>) -> Option<()> {
    consume_until(lexer, "%}")
}

fn template_expr(lexer: &mut Lexer<'_, RawToken>) -> Option<()> {
    consume_until(lexer, "}}")
}

fn consume_until(lexer: &mut Lexer<'_, RawToken>, needle: &str) -> Option<()> {
    if let Some(idx) = lexer.remainder().find(needle) {
        lexer.bump(idx + needle.len());
        Some(())
    } else {
        lexer.bump(lexer.remainder().len());
        None
    }
}

#[derive(Debug, Clone)]
struct PositionTracker<'source> {
    source: &'source str,
    offset: usize,
    line: usize,
    column: usize,
}

impl<'source> PositionTracker<'source> {
    fn new(source: &'source str) -> Self {
        Self {
            source,
            offset: 0,
            line: 1,
            column: 1,
        }
    }

    fn record(&mut self, span: Range<usize>) -> TokenSpan {
        let (start_line, start_column) = self.advance_to(span.start);
        let (end_line, end_column) = self.advance_to(span.end);
        TokenSpan::new(span.start, span.end, start_line, start_column, end_line, end_column)
    }

    fn advance_to(&mut self, target: usize) -> (usize, usize) {
        if target < self.offset {
            return (self.line, self.column);
        }
        for ch in self.source[self.offset..target].chars() {
            if ch == '\n' {
                self.line += 1;
                self.column = 1;
            } else {
                self.column += 1;
            }
        }
        self.offset = target;
        (self.line, self.column)
    }
}
