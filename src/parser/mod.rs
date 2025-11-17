//! Parser that lifts tokenizer output into the AST definitions.

use crate::{
    ast::{
        DtComment, DtConditional, DtConditionalBranch, DtDirective, DtInclude, DtItem, DtMacro,
        DtMacroCall, DtNode, DtProperty, DtTemplate, DtValue, TemplateKind,
    },
    tokenizer::{LayoutError, Token, TokenKind, TokenSpan, TokenStream},
};

/// Parse Devicetree text into the ordered list of AST items.
pub fn parse_layout(source: &str) -> Result<Vec<DtItem>, LayoutError> {
    Parser::new(source)?.parse()
}

struct Parser<'a> {
    source: &'a str,
    tokens: Vec<Token>,
    idx: usize,
    pending_comments: Vec<DtComment>,
}

impl<'a> Parser<'a> {
    fn new(source: &'a str) -> Result<Self, LayoutError> {
        let tokens = TokenStream::new(source)
            .with_trivia(true)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            source,
            tokens,
            idx: 0,
            pending_comments: Vec::new(),
        })
    }

    fn parse(mut self) -> Result<Vec<DtItem>, LayoutError> {
        let mut items = Vec::new();
        while self.consume_trivia() {
            if self.is_eof() {
                break;
            }
            let leading = self.take_pending_comments();
            if let Some(item) = self.parse_item_with_leading(leading)? {
                items.push(item);
            }
        }

        for comment in self.take_pending_comments() {
            items.push(DtItem::Comment(comment));
        }
        Ok(items)
    }

    fn parse_item_with_leading(
        &mut self,
        leading: Vec<DtComment>,
    ) -> Result<Option<DtItem>, LayoutError> {
        let token = match self.peek() {
            Some(token) => token.clone(),
            None => return Ok(None),
        };

        let item = match token.kind {
            TokenKind::Identifier | TokenKind::Reference => {
                let ident = self.next().unwrap();
                let next_kind = self
                    .peek_kind_skipping_templates()
                    .ok_or_else(|| self.error(ident.span, "expected `{`, `=`, or macro call"))?;
                if next_kind == TokenKind::LBrace {
                    DtItem::Node(self.parse_node(ident, leading)?)
                } else if next_kind == TokenKind::Equals {
                    DtItem::Property(self.parse_property(ident, leading)?)
                } else if next_kind == TokenKind::Semicolon {
                    DtItem::Property(self.parse_flag_property(ident, leading)?)
                } else if next_kind == TokenKind::LParen {
                    DtItem::MacroCall(self.parse_macro_call(ident, leading)?)
                } else {
                    return Err(self.error(ident.span, "unexpected identifier context"));
                }
            }
            TokenKind::PreprocessorInclude => DtItem::Include(self.parse_include(leading)?),
            TokenKind::PreprocessorDefine => DtItem::Macro(self.parse_macro(leading)?),
            TokenKind::PreprocessorOther => {
                let directive = self.next().unwrap();
                let trimmed = directive.lexeme.trim();
                if Self::is_if_like(trimmed) {
                    DtItem::Conditional(self.parse_conditional_from_token(directive, leading)?)
                } else {
                    DtItem::Directive(DtDirective {
                        text: directive.lexeme,
                        span: directive.span,
                        leading_comments: leading,
                    })
                }
            }
            TokenKind::TemplateBlock | TokenKind::TemplateExpr => {
                DtItem::Template(self.parse_template(leading)?)
            }
            other => {
                return Err(self.error(
                    token.span,
                    format!("unexpected token {:?} at top level", other),
                ));
            }
        };
        Ok(Some(item))
    }

    fn parse_node(
        &mut self,
        name_token: Token,
        leading: Vec<DtComment>,
    ) -> Result<DtNode, LayoutError> {
        let Token {
            lexeme: mut node_name,
            span: mut name_span,
            ..
        } = name_token;

        let raw_start_span = name_span;
        if self.match_token(TokenKind::Colon).is_some() {
            self.consume_whitespace();
            let next = self
                .next()
                .ok_or_else(|| self.error(name_span, "expected node name after label"))?;
            match next.kind {
                TokenKind::Identifier | TokenKind::Reference => {
                    node_name = next.lexeme;
                    name_span = next.span;
                }
                _ => {
                    return Err(self.error(next.span, "expected node name after label"));
                }
            }
        }

        let (raw_name, brace_span) = self.collect_raw_until(raw_start_span, TokenKind::LBrace)?;
        let brace = self.expect(TokenKind::LBrace, "expected `{` after node name")?;
        let mut node = DtNode {
            name: node_name.clone(),
            raw_name: if raw_name.is_empty() {
                node_name.clone()
            } else {
                raw_name
            },
            span: merge_spans(name_span, brace.span),
            properties: Vec::new(),
            children: Vec::new(),
            leading_comments: leading,
            trailing_comments: Vec::new(),
        };

        loop {
            self.consume_trivia();
            if self.is_eof() {
                return Err(self.error(brace_span, "unterminated node body"));
            }
            if self.peek_kind() == Some(TokenKind::RBrace) {
                let end = self.next().unwrap();
                self.consume_whitespace();
                let semicolon = self.match_token(TokenKind::Semicolon);
                let end_span = semicolon.as_ref().map(|tok| tok.span).unwrap_or(end.span);
                node.span = merge_spans(name_span, end_span);
                node.trailing_comments = self.take_pending_comments();
                break;
            }

            if self.peek().is_none() {
                return Err(self.error(brace_span, "unterminated node"));
            }

            let child_leading = self.take_pending_comments();
            match self.peek_kind() {
                Some(TokenKind::Identifier | TokenKind::Reference) => {
                    let ident = self.next().unwrap();
                    let next_kind = self
                        .peek_kind_skipping_templates()
                        .ok_or_else(|| self.error(ident.span, "expected `{`, `=` or macro call"))?;
                    if next_kind == TokenKind::LBrace {
                        let child = self.parse_node(ident, child_leading)?;
                        node.children.push(DtItem::Node(child));
                    } else if next_kind == TokenKind::Equals {
                        let prop = self.parse_property(ident, child_leading)?;
                        node.properties.push(prop);
                    } else if next_kind == TokenKind::Semicolon {
                        let flag = self.parse_flag_property(ident, child_leading)?;
                        node.properties.push(flag);
                    } else if next_kind == TokenKind::LParen {
                        let call = self.parse_macro_call(ident, child_leading)?;
                        node.children.push(DtItem::MacroCall(call));
                    } else {
                        return Err(
                            self.error(ident.span, "expected node, property, or macro call")
                        );
                    }
                }
                Some(TokenKind::HashIdentifier) => {
                    let ident = self.next().unwrap();
                    let prop = self.parse_property(ident, child_leading)?;
                    node.properties.push(prop);
                }
                Some(TokenKind::TemplateBlock | TokenKind::TemplateExpr) => {
                    let template = self.parse_template(child_leading)?;
                    node.children.push(DtItem::Template(template));
                }
                Some(TokenKind::PreprocessorInclude) => {
                    node.children
                        .push(DtItem::Include(self.parse_include(child_leading)?));
                }
                Some(TokenKind::PreprocessorDefine) => {
                    node.children
                        .push(DtItem::Macro(self.parse_macro(child_leading)?));
                }
                Some(TokenKind::PreprocessorOther) => {
                    let directive = self.next().unwrap();
                    if Self::is_if_like(directive.lexeme.trim()) {
                        let conditional =
                            self.parse_conditional_from_token(directive, child_leading)?;
                        node.children.push(DtItem::Conditional(conditional));
                    } else {
                        return Err(self.error(directive.span, "unexpected directive inside node"));
                    }
                }
                Some(other) => {
                    return Err(self.error(
                        self.peek().unwrap().span,
                        format!("unexpected token {:?} in node", other),
                    ));
                }
                None => break,
            }
        }

        Ok(node)
    }

    fn parse_property(
        &mut self,
        name_token: Token,
        leading: Vec<DtComment>,
    ) -> Result<DtProperty, LayoutError> {
        let Token {
            lexeme: name_text,
            span: name_span,
            ..
        } = name_token;
        let (raw_name, _) = self.collect_raw_until(name_span, TokenKind::Equals)?;
        self.expect(TokenKind::Equals, "expected '=' after property name")?;
        let value = self.parse_property_value()?;
        let semicolon = self.expect(TokenKind::Semicolon, "expected ';' after property")?;
        let trailing_comment = self.capture_inline_comment();
        let span = merge_spans(name_span, semicolon.span);
        Ok(DtProperty {
            name: name_text.clone(),
            raw_name: if raw_name.is_empty() {
                name_text
            } else {
                raw_name
            },
            value,
            span,
            leading_comments: leading,
            trailing_comment,
        })
    }

    fn parse_flag_property(
        &mut self,
        name_token: Token,
        leading: Vec<DtComment>,
    ) -> Result<DtProperty, LayoutError> {
        let name = name_token.lexeme.clone();
        let span = name_token.span;
        self.consume_whitespace();
        let semicolon = self.expect(TokenKind::Semicolon, "expected ';' after flag property")?;
        let trailing_comment = self.capture_inline_comment();
        Ok(DtProperty {
            name: name.clone(),
            raw_name: name,
            value: DtValue {
                raw: String::new(),
                span,
            },
            span: merge_spans(span, semicolon.span),
            leading_comments: leading,
            trailing_comment,
        })
    }

    fn parse_property_value(&mut self) -> Result<DtValue, LayoutError> {
        if self.is_eof() {
            return Err(self.error(TokenSpan::new(0, 0, 1, 1, 1, 1), "expected property value"));
        }

        let mut start_idx = self.idx;
        while let Some(token) = self.tokens.get(start_idx) {
            if token.kind == TokenKind::Whitespace && !token.lexeme.contains('\n') {
                start_idx += 1;
                continue;
            }
            break;
        }

        if start_idx >= self.tokens.len() {
            return Err(self.error(TokenSpan::new(0, 0, 1, 1, 1, 1), "expected value token"));
        }

        let start_token = self.tokens[start_idx].clone();
        let mut depth_angles = 0usize;
        let mut depth_braces = 0usize;
        let mut last_span = start_token.span;
        let mut idx = start_idx;

        while let Some(token) = self.tokens.get(idx) {
            match token.kind {
                TokenKind::Semicolon if depth_angles == 0 && depth_braces == 0 => break,
                TokenKind::AngleOpen => {
                    depth_angles += 1;
                }
                TokenKind::AngleClose => {
                    if depth_angles > 0 {
                        depth_angles -= 1;
                    }
                }
                TokenKind::LBrace => {
                    depth_braces += 1;
                }
                TokenKind::RBrace => {
                    if depth_braces == 0 {
                        break;
                    }
                    depth_braces -= 1;
                }
                _ => {}
            }
            last_span = token.span;
            idx += 1;
            if idx >= self.tokens.len() {
                break;
            }
        }

        if idx == start_idx {
            return Err(self.error(start_token.span, "missing property value"));
        }

        let span = merge_spans(start_token.span, last_span);
        let raw = self.source[span.start..span.end].to_string();
        self.idx = idx;
        Ok(DtValue { raw, span })
    }

    fn parse_include(&mut self, leading: Vec<DtComment>) -> Result<DtInclude, LayoutError> {
        let token = self.next().unwrap();
        Ok(DtInclude {
            text: token.lexeme,
            span: token.span,
            leading_comments: leading,
        })
    }

    fn parse_macro(&mut self, leading: Vec<DtComment>) -> Result<DtMacro, LayoutError> {
        let token = self.next().unwrap();
        Ok(DtMacro {
            text: token.lexeme,
            span: token.span,
            leading_comments: leading,
        })
    }

    fn parse_macro_call(
        &mut self,
        name_token: Token,
        leading: Vec<DtComment>,
    ) -> Result<DtMacroCall, LayoutError> {
        let mut last_span = name_token.span;
        let mut depth = 0usize;
        while let Some(token) = self.peek() {
            last_span = token.span;
            match token.kind {
                TokenKind::LParen => {
                    depth += 1;
                    self.idx += 1;
                }
                TokenKind::RParen => {
                    self.idx += 1;
                    if depth == 0 {
                        break;
                    }
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {
                    self.idx += 1;
                }
            }
        }
        let text = self.source[name_token.span.start..last_span.end]
            .trim_end()
            .to_string();
        Ok(DtMacroCall {
            text,
            span: merge_spans(name_token.span, last_span),
            leading_comments: leading,
        })
    }

    fn parse_template(&mut self, leading: Vec<DtComment>) -> Result<DtTemplate, LayoutError> {
        let token = self.next().unwrap();
        let kind = match token.kind {
            TokenKind::TemplateBlock => TemplateKind::Block,
            TokenKind::TemplateExpr => TemplateKind::Expression,
            _ => unreachable!(),
        };
        Ok(DtTemplate {
            raw: token.lexeme,
            kind,
            span: token.span,
            leading_comments: leading,
        })
    }

    fn parse_conditional_from_token(
        &mut self,
        start: Token,
        leading: Vec<DtComment>,
    ) -> Result<DtConditional, LayoutError> {
        let mut branches = vec![DtConditionalBranch {
            directive: DtDirective {
                text: start.lexeme,
                span: start.span,
                leading_comments: leading,
            },
            items: Vec::new(),
        }];

        loop {
            self.consume_trivia();
            if self.is_eof() {
                return Err(self.error(start.span, "unterminated conditional"));
            }

            let leading = self.take_pending_comments();

            let token = match self.peek() {
                Some(token) => token.clone(),
                None => break,
            };

            match token.kind {
                TokenKind::PreprocessorOther => {
                    let directive = self.next().unwrap();
                    let trimmed = directive.lexeme.trim();
                    if trimmed.starts_with("#endif") {
                        return Ok(DtConditional {
                            branches,
                            end_directive: DtDirective {
                                text: directive.lexeme,
                                span: directive.span,
                                leading_comments: leading,
                            },
                        });
                    } else if trimmed.starts_with("#else") || trimmed.starts_with("#elif") {
                        branches.push(DtConditionalBranch {
                            directive: DtDirective {
                                text: directive.lexeme,
                                span: directive.span,
                                leading_comments: leading,
                            },
                            items: Vec::new(),
                        });
                    } else if Self::is_if_like(trimmed) {
                        let nested = self.parse_conditional_from_token(directive, leading)?;
                        branches
                            .last_mut()
                            .expect("branch exists")
                            .items
                            .push(DtItem::Conditional(nested));
                    } else {
                        branches
                            .last_mut()
                            .expect("branch exists")
                            .items
                            .push(DtItem::Comment(DtComment::new(
                                directive.lexeme,
                                directive.span,
                            )));
                    }
                }
                _ => {
                    if let Some(item) = self.parse_item_with_leading(leading)? {
                        branches.last_mut().expect("branch exists").items.push(item);
                    }
                }
            }
        }
        Err(self.error(start.span, "unterminated conditional"))
    }

    fn consume_trivia(&mut self) -> bool {
        let mut advanced = false;
        while let Some(token) = self.peek() {
            match token.kind {
                TokenKind::Whitespace => {
                    self.idx += 1;
                    advanced = true;
                }
                TokenKind::LineComment | TokenKind::BlockComment => {
                    let comment = self.next().unwrap();
                    self.pending_comments
                        .push(DtComment::new(comment.lexeme, comment.span));
                    advanced = true;
                }
                _ => break,
            }
        }
        advanced || !self.is_eof()
    }

    fn consume_whitespace(&mut self) {
        while let Some(token) = self.peek() {
            if token.kind == TokenKind::Whitespace {
                self.idx += 1;
            } else {
                break;
            }
        }
    }

    fn capture_inline_comment(&mut self) -> Option<DtComment> {
        let mut idx = self.idx;
        let mut spacing = String::new();
        while let Some(token) = self.tokens.get(idx) {
            match token.kind {
                TokenKind::Whitespace => {
                    if token.lexeme.contains('\n') {
                        return None;
                    }
                    spacing.push_str(&token.lexeme);
                    idx += 1;
                }
                TokenKind::LineComment | TokenKind::BlockComment => {
                    let comment = token.clone();
                    self.idx = idx + 1;
                    let mut text = spacing;
                    text.push_str(&comment.lexeme);
                    return Some(DtComment::new(text, comment.span));
                }
                _ => break,
            }
        }
        None
    }

    fn collect_raw_until(
        &mut self,
        start_span: TokenSpan,
        terminator: TokenKind,
    ) -> Result<(String, TokenSpan), LayoutError> {
        let mut idx = self.idx;
        while let Some(token) = self.tokens.get(idx) {
            match token.kind {
                TokenKind::Whitespace | TokenKind::TemplateExpr => {
                    idx += 1;
                    continue;
                }
                _ if token.kind == terminator => {
                    let raw = self.source[start_span.start..token.span.start]
                        .trim_end()
                        .to_string();
                    while self.idx < idx {
                        self.idx += 1;
                    }
                    return Ok((raw, token.span));
                }
                _ => break,
            }
        }
        Err(self.error(start_span, format!("expected {:?}", terminator)))
    }

    fn peek_kind(&self) -> Option<TokenKind> {
        self.peek().map(|token| token.kind)
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.idx)
    }

    fn peek_kind_skipping_templates(&self) -> Option<TokenKind> {
        let mut idx = self.idx;
        while let Some(token) = self.tokens.get(idx) {
            match token.kind {
                TokenKind::Whitespace | TokenKind::TemplateExpr => idx += 1,
                TokenKind::Colon => {
                    let mut lookahead = idx + 1;
                    loop {
                        match self.tokens.get(lookahead) {
                            Some(tok)
                                if matches!(
                                    tok.kind,
                                    TokenKind::Whitespace | TokenKind::TemplateExpr
                                ) =>
                            {
                                lookahead += 1;
                            }
                            Some(tok)
                                if matches!(
                                    tok.kind,
                                    TokenKind::Identifier | TokenKind::Reference
                                ) =>
                            {
                                idx = lookahead + 1;
                                break;
                            }
                            Some(tok) => return Some(tok.kind),
                            None => return None,
                        }
                    }
                }
                _ => return Some(token.kind),
            }
        }
        None
    }

    fn next(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.idx).cloned();
        if token.is_some() {
            self.idx += 1;
        }
        token
    }

    fn match_token(&mut self, kind: TokenKind) -> Option<Token> {
        if self.peek_kind() == Some(kind) {
            self.next()
        } else {
            None
        }
    }

    fn expect(&mut self, kind: TokenKind, msg: &str) -> Result<Token, LayoutError> {
        match self.next() {
            Some(token) if token.kind == kind => Ok(token),
            Some(token) => Err(self.error(token.span, msg)),
            None => Err(self.error(
                TokenSpan::new(0, 0, 1, 1, 1, 1),
                format!("expected {:?}", kind),
            )),
        }
    }

    fn take_pending_comments(&mut self) -> Vec<DtComment> {
        std::mem::take(&mut self.pending_comments)
    }

    fn is_eof(&self) -> bool {
        self.idx >= self.tokens.len()
    }

    fn error(&self, span: TokenSpan, message: impl Into<String>) -> LayoutError {
        LayoutError::Parse {
            message: message.into(),
            span,
        }
    }

    fn is_if_like(text: &str) -> bool {
        text.starts_with("#if")
    }
}

fn merge_spans(a: TokenSpan, b: TokenSpan) -> TokenSpan {
    let (start, start_line, start_column) = if a.start <= b.start {
        (a.start, a.start_line, a.start_column)
    } else {
        (b.start, b.start_line, b.start_column)
    };
    let (end, end_line, end_column) = if a.end >= b.end {
        (a.end, a.end_line, a.end_column)
    } else {
        (b.end, b.end_line, b.end_column)
    };
    TokenSpan::new(start, end, start_line, start_column, end_line, end_column)
}
