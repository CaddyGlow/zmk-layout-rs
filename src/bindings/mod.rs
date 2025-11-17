//! Binding parser and data structures for ZMK behaviors.

use std::fmt;

/// Parameter value represented as either text or integer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParamValue {
    Text(String),
    Integer(i64),
}

impl ParamValue {
    fn to_string(&self) -> String {
        match self {
            ParamValue::Text(text) => text.clone(),
            ParamValue::Integer(value) => value.to_string(),
        }
    }
}

/// Nested parameter used by layout bindings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutParam {
    pub value: ParamValue,
    pub params: Vec<LayoutParam>,
}

impl LayoutParam {
    pub fn new(value: ParamValue, params: Vec<LayoutParam>) -> Self {
        Self { value, params }
    }

    fn as_expression(&self) -> String {
        if self.params.is_empty() {
            self.value.to_string()
        } else {
            let inner = self
                .params
                .iter()
                .map(|param| param.as_expression())
                .collect::<Vec<_>>()
                .join(",");
            format!("{}({inner})", self.value.to_string())
        }
    }

    fn text(expr: String) -> Self {
        Self {
            value: ParamValue::Text(expr),
            params: Vec::new(),
        }
    }
}

/// Layout binding describing a behavior and its parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutBinding {
    pub value: String,
    pub params: Vec<LayoutParam>,
}

impl LayoutBinding {
    pub fn none() -> Self {
        Self {
            value: "&none".to_string(),
            params: Vec::new(),
        }
    }
}

/// Parser that converts binding strings into [`LayoutBinding`] structures.
#[derive(Debug, Default)]
pub struct BindingParser;

impl BindingParser {
    pub fn new() -> Self {
        Self
    }

    pub fn parse(&self, source: &str) -> LayoutBinding {
        self.parse_internal(source)
            .unwrap_or_else(|_| LayoutBinding::none())
    }

    pub fn parse_with_behavior_rules(&self, source: &str) -> LayoutBinding {
        let mut binding = self.parse(source);
        let behavior = binding.value.to_lowercase();
        let mut flat_behaviors = vec!["&mt", "&lt", "&caps_word"];
        if behavior.contains("&hrm_")
            || behavior.contains("&caps")
            || behavior.contains("&thumb")
            || behavior.contains("&space")
        {
            flat_behaviors.push(&behavior);
        }

        if flat_behaviors
            .iter()
            .any(|candidate| candidate == &behavior)
        {
            binding = flatten_params(binding);
        } else if behavior == "&kp" || behavior == "&key_repeat" {
            binding = handle_modifier_chain(binding);
        }
        binding
    }

    fn parse_internal(&self, source: &str) -> Result<LayoutBinding, ()> {
        let trimmed = source.trim();
        if trimmed.is_empty() {
            return Ok(LayoutBinding::none());
        }
        let tokens = tokenize(trimmed)?;
        let mut parser = Parser::new(tokens);
        parser.parse_binding()
    }
}

fn flatten_params(binding: LayoutBinding) -> LayoutBinding {
    fn recurse(param: &LayoutParam, acc: &mut Vec<LayoutParam>) {
        acc.push(LayoutParam::new(param.value.clone(), Vec::new()));
        for nested in &param.params {
            recurse(nested, acc);
        }
    }

    let mut flat = Vec::new();
    for param in &binding.params {
        recurse(param, &mut flat);
    }
    LayoutBinding {
        value: binding.value,
        params: flat,
    }
}

fn handle_modifier_chain(binding: LayoutBinding) -> LayoutBinding {
    if binding.params.len() <= 1 {
        return binding;
    }

    const MODIFIERS: [&str; 8] = ["lc", "la", "lg", "ls", "rc", "ra", "rg", "rs"];
    let first = &binding.params[0];
    let value = match &first.value {
        ParamValue::Text(text) => text.clone(),
        ParamValue::Integer(_) => return binding,
    };
    if !MODIFIERS.contains(&value.to_lowercase().as_str()) {
        return binding;
    }

    let mut iter = binding.params.iter();
    let mut result = iter
        .next_back()
        .cloned()
        .unwrap_or_else(|| LayoutParam::text("".into()));
    while let Some(param) = iter.next_back() {
        result = LayoutParam::new(param.value.clone(), vec![result]);
    }
    LayoutBinding {
        value: binding.value,
        params: vec![result],
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Token {
    Ampersand,
    Identifier(String),
    Number(i64),
    Word(String),
    ShiftLeft,
    ShiftRight,
    BitOr,
    BitXor,
    LParen,
    RParen,
    Minus,
}

fn tokenize(source: &str) -> Result<Vec<Token>, ()> {
    let mut chars = source.chars().peekable();
    let mut tokens = Vec::new();
    while let Some(&ch) = chars.peek() {
        match ch {
            ' ' | '\t' | '\n' | '\r' => {
                chars.next();
            }
            '&' => {
                chars.next();
                tokens.push(Token::Ampersand);
            }
            '(' => {
                chars.next();
                tokens.push(Token::LParen);
            }
            ')' => {
                chars.next();
                tokens.push(Token::RParen);
            }
            '<' => {
                chars.next();
                if chars.peek() == Some(&'<') {
                    chars.next();
                    tokens.push(Token::ShiftLeft);
                } else {
                    return Err(());
                }
            }
            '>' => {
                chars.next();
                if chars.peek() == Some(&'>') {
                    chars.next();
                    tokens.push(Token::ShiftRight);
                } else {
                    return Err(());
                }
            }
            '|' => {
                chars.next();
                tokens.push(Token::BitOr);
            }
            '^' => {
                chars.next();
                tokens.push(Token::BitXor);
            }
            '-' => {
                chars.next();
                if let Some(Token::Number(_)) | Some(Token::Word(_)) = tokens.last() {
                    tokens.push(Token::Minus);
                } else if let Some(next) = chars.peek() {
                    if next.is_ascii_digit() {
                        let mut buf = String::from("-");
                        while let Some(&digit) = chars.peek() {
                            if digit.is_ascii_digit() {
                                buf.push(digit);
                                chars.next();
                            } else {
                                break;
                            }
                        }
                        let value = buf.parse().map_err(|_| ())?;
                        tokens.push(Token::Number(value));
                    } else {
                        tokens.push(Token::Minus);
                    }
                } else {
                    tokens.push(Token::Minus);
                }
            }
            ch if ch.is_ascii_digit() => {
                let mut buf = String::new();
                while let Some(&digit) = chars.peek() {
                    if digit.is_ascii_hexdigit() || digit == 'x' || digit == 'X' {
                        buf.push(digit);
                        chars.next();
                    } else {
                        break;
                    }
                }
                if buf.starts_with("0x") || buf.starts_with("0X") {
                    if let Ok(value) = i64::from_str_radix(&buf[2..], 16) {
                        tokens.push(Token::Number(value));
                    } else {
                        tokens.push(Token::Word(buf));
                    }
                } else if let Ok(value) = buf.parse() {
                    tokens.push(Token::Number(value));
                } else {
                    tokens.push(Token::Word(buf));
                }
            }
            ch if ch.is_ascii_alphabetic() || ch == '_' => {
                let mut buf = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_ascii_alphanumeric() || c == '_' {
                        buf.push(c);
                        chars.next();
                    } else {
                        break;
                    }
                }
                tokens.push(Token::Identifier(buf));
            }
            _ => return Err(()),
        }
    }
    Ok(tokens)
}

struct Parser {
    tokens: Vec<Token>,
    idx: usize,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, idx: 0 }
    }

    fn parse_binding(&mut self) -> Result<LayoutBinding, ()> {
        let behavior = self.parse_behavior()?;
        let mut params = Vec::new();
        while self.peek().is_some() {
            params.push(self.parse_param()?);
        }
        Ok(LayoutBinding {
            value: behavior,
            params,
        })
    }

    fn parse_behavior(&mut self) -> Result<String, ()> {
        let mut has_amp = false;
        if matches!(self.peek(), Some(Token::Ampersand)) {
            has_amp = true;
            self.next();
        }
        let name = match self.next() {
            Some(Token::Identifier(name)) => name,
            Some(Token::Word(name)) => name,
            _ => return Err(()),
        };
        let mut behavior = name;
        if !has_amp {
            behavior = format!("&{behavior}");
        } else if !behavior.starts_with('&') {
            behavior = format!("&{behavior}");
        }
        Ok(behavior)
    }

    fn parse_param(&mut self) -> Result<LayoutParam, ()> {
        self.parse_shift_expr()
    }

    fn parse_shift_expr(&mut self) -> Result<LayoutParam, ()> {
        let mut left = self.parse_and_expr()?;
        loop {
            match self.peek() {
                Some(Token::ShiftLeft) => {
                    self.next();
                    let right = self.parse_and_expr()?;
                    left = LayoutParam::text(format!(
                        "({}) << ({})",
                        left.as_expression(),
                        right.as_expression()
                    ));
                }
                Some(Token::ShiftRight) => {
                    self.next();
                    let right = self.parse_and_expr()?;
                    left = LayoutParam::text(format!(
                        "({}) >> ({})",
                        left.as_expression(),
                        right.as_expression()
                    ));
                }
                _ => break,
            }
        }
        Ok(left)
    }

    fn parse_and_expr(&mut self) -> Result<LayoutParam, ()> {
        let mut left = self.parse_or_expr()?;
        while matches!(self.peek(), Some(Token::Ampersand)) {
            self.next();
            let right = self.parse_or_expr()?;
            left = LayoutParam::text(format!(
                "({}) & ({})",
                left.as_expression(),
                right.as_expression()
            ));
        }
        Ok(left)
    }

    fn parse_or_expr(&mut self) -> Result<LayoutParam, ()> {
        let mut left = self.parse_xor_expr()?;
        while matches!(self.peek(), Some(Token::BitOr)) {
            self.next();
            let right = self.parse_xor_expr()?;
            left = LayoutParam::text(format!(
                "({}) | ({})",
                left.as_expression(),
                right.as_expression()
            ));
        }
        Ok(left)
    }

    fn parse_xor_expr(&mut self) -> Result<LayoutParam, ()> {
        let mut left = self.parse_primary()?;
        while matches!(self.peek(), Some(Token::BitXor)) {
            self.next();
            let right = self.parse_primary()?;
            left = LayoutParam::text(format!(
                "({}) ^ ({})",
                left.as_expression(),
                right.as_expression()
            ));
        }
        Ok(left)
    }

    fn parse_primary(&mut self) -> Result<LayoutParam, ()> {
        match self.next() {
            Some(Token::LParen) => {
                let expr = self.parse_shift_expr()?;
                self.expect(Token::RParen)?;
                Ok(LayoutParam::text(format!("({})", expr.as_expression())))
            }
            Some(Token::Minus) => {
                let value = self.parse_primary()?;
                match value.value {
                    ParamValue::Integer(n) => {
                        Ok(LayoutParam::new(ParamValue::Integer(-n), Vec::new()))
                    }
                    _ => Ok(LayoutParam::text(format!("-{}", value.as_expression()))),
                }
            }
            Some(Token::Number(value)) => {
                Ok(LayoutParam::new(ParamValue::Integer(value), Vec::new()))
            }
            Some(Token::Identifier(name)) => {
                if matches!(self.peek(), Some(Token::LParen)) {
                    self.next();
                    let mut params = Vec::new();
                    while !matches!(self.peek(), Some(Token::RParen)) {
                        params.push(self.parse_param()?);
                    }
                    self.expect(Token::RParen)?;
                    Ok(LayoutParam::new(ParamValue::Text(name), params))
                } else {
                    Ok(LayoutParam::new(parse_param_value(&name), Vec::new()))
                }
            }
            Some(Token::Word(word)) => Ok(LayoutParam::new(parse_param_value(&word), Vec::new())),
            Some(Token::Ampersand) => {
                let ident = match self.next() {
                    Some(Token::Identifier(name)) => name,
                    Some(Token::Word(name)) => name,
                    _ => return Err(()),
                };
                let mut params = Vec::new();
                while let Some(token) = self.peek() {
                    if matches!(token, Token::RParen) {
                        break;
                    }
                    params.push(self.parse_param()?);
                    if matches!(self.peek(), Some(Token::RParen)) {
                        break;
                    }
                    if matches!(self.peek(), None) {
                        break;
                    }
                }
                Ok(LayoutParam::new(
                    ParamValue::Text(format!("&{ident}")),
                    params,
                ))
            }
            _ => Err(()),
        }
    }

    fn expect(&mut self, expected: Token) -> Result<(), ()> {
        if self.next() == Some(expected) {
            Ok(())
        } else {
            Err(())
        }
    }

    fn next(&mut self) -> Option<Token> {
        if self.idx >= self.tokens.len() {
            None
        } else {
            let token = self.tokens[self.idx].clone();
            self.idx += 1;
            Some(token)
        }
    }

    fn peek(&self) -> Option<Token> {
        self.tokens.get(self.idx).cloned()
    }
}

fn parse_param_value(value: &str) -> ParamValue {
    if let Ok(num) = value.parse::<i64>() {
        ParamValue::Integer(num)
    } else if let Some(rest) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        if let Ok(num) = i64::from_str_radix(rest, 16) {
            return ParamValue::Integer(num);
        }
        ParamValue::Text(value.to_string())
    } else {
        ParamValue::Text(value.to_string())
    }
}

impl fmt::Display for LayoutBinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.value)
    }
}
