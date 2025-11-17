//! Abstract syntax tree structures produced by the parser.

use crate::tokenizer::TokenSpan;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DtItem {
    Node(DtNode),
    Conditional(DtConditional),
    Macro(DtMacro),
    MacroCall(DtMacroCall),
    Include(DtInclude),
    Template(DtTemplate),
    Comment(DtComment),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DtNode {
    pub name: String,
    pub raw_name: String,
    pub span: TokenSpan,
    pub properties: Vec<DtProperty>,
    pub children: Vec<DtItem>,
    pub leading_comments: Vec<DtComment>,
    pub trailing_comments: Vec<DtComment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DtProperty {
    pub name: String,
    pub raw_name: String,
    pub value: DtValue,
    pub span: TokenSpan,
    pub leading_comments: Vec<DtComment>,
    pub trailing_comment: Option<DtComment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DtValue {
    pub raw: String,
    pub span: TokenSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DtComment {
    pub text: String,
    pub span: TokenSpan,
}

impl DtComment {
    pub fn new(text: String, span: TokenSpan) -> Self {
        Self { text, span }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DtTemplate {
    pub raw: String,
    pub kind: TemplateKind,
    pub span: TokenSpan,
    pub leading_comments: Vec<DtComment>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplateKind {
    Block,
    Expression,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DtMacro {
    pub text: String,
    pub span: TokenSpan,
    pub leading_comments: Vec<DtComment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DtMacroCall {
    pub text: String,
    pub span: TokenSpan,
    pub leading_comments: Vec<DtComment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DtInclude {
    pub text: String,
    pub span: TokenSpan,
    pub leading_comments: Vec<DtComment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DtConditional {
    pub branches: Vec<DtConditionalBranch>,
    pub end_directive: DtDirective,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DtConditionalBranch {
    pub directive: DtDirective,
    pub items: Vec<DtItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DtDirective {
    pub text: String,
    pub span: TokenSpan,
    pub leading_comments: Vec<DtComment>,
}
