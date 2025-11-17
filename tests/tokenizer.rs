use zmk_layout_rs::tokenizer::{LayoutError, TokenKind, TokenStream};

#[test]
fn tokenize_simple_block() -> Result<(), LayoutError> {
    let tokens = TokenStream::new("keymap { };").collect::<Result<Vec<_>, _>>()?;
    let kinds: Vec<_> = tokens.iter().map(|token| token.kind).collect();
    assert_eq!(
        kinds,
        vec![
            TokenKind::Identifier,
            TokenKind::LBrace,
            TokenKind::RBrace,
            TokenKind::Semicolon,
        ]
    );

    let ident = &tokens[0];
    assert_eq!(ident.lexeme, "keymap");
    assert_eq!(ident.span.start, 0);
    assert_eq!(ident.span.end, 6);
    assert_eq!(ident.span.start_line, 1);
    assert_eq!(ident.span.start_column, 1);

    let rbrace = &tokens[2];
    assert_eq!(rbrace.span.start, 9);
    assert_eq!(rbrace.span.start_line, 1);
    assert_eq!(rbrace.span.start_column, 10);
    Ok(())
}

#[test]
fn tokenize_angle_brackets_with_trivia() -> Result<(), LayoutError> {
    let source = "< &kp Q &kp W >;";
    let tokens = TokenStream::new(source)
        .with_trivia(true)
        .collect::<Result<Vec<_>, _>>()?;
    let kinds: Vec<_> = tokens.iter().map(|token| token.kind).collect();
    assert_eq!(
        kinds,
        vec![
            TokenKind::AngleOpen,
            TokenKind::Whitespace,
            TokenKind::Reference,
            TokenKind::Whitespace,
            TokenKind::Identifier,
            TokenKind::Whitespace,
            TokenKind::Reference,
            TokenKind::Whitespace,
            TokenKind::Identifier,
            TokenKind::Whitespace,
            TokenKind::AngleClose,
            TokenKind::Semicolon,
        ]
    );

    let whitespace = tokens[1].clone();
    assert_eq!(whitespace.lexeme, " ");
    Ok(())
}

#[test]
fn tokenize_comments() -> Result<(), LayoutError> {
    let source = "// base layer\n/* block comment */";
    let tokens = TokenStream::new(source).collect::<Result<Vec<_>, _>>()?;
    let kinds: Vec<_> = tokens.iter().map(|token| token.kind).collect();
    assert_eq!(kinds, vec![TokenKind::LineComment, TokenKind::BlockComment]);
    assert_eq!(tokens[0].lexeme, "// base layer");
    assert_eq!(tokens[1].lexeme, "/* block comment */");
    Ok(())
}

#[test]
fn tokenize_preprocessor_directives() -> Result<(), LayoutError> {
    let source = "#include <foo>\n#define MACRO(x)";
    let tokens = TokenStream::new(source).collect::<Result<Vec<_>, _>>()?;
    let kinds: Vec<_> = tokens.iter().map(|token| token.kind).collect();
    assert_eq!(
        kinds,
        vec![
            TokenKind::PreprocessorInclude,
            TokenKind::PreprocessorDefine
        ]
    );
    assert_eq!(tokens[0].lexeme, "#include <foo>");
    assert_eq!(tokens[1].lexeme, "#define MACRO(x)");
    Ok(())
}

#[test]
fn tokenize_template_tags() -> Result<(), LayoutError> {
    let source = "{% for layer %}{{ value }}";
    let tokens = TokenStream::new(source).collect::<Result<Vec<_>, _>>()?;
    let kinds: Vec<_> = tokens.iter().map(|token| token.kind).collect();
    assert_eq!(
        kinds,
        vec![TokenKind::TemplateBlock, TokenKind::TemplateExpr]
    );
    assert_eq!(tokens[0].lexeme, "{% for layer %}");
    assert_eq!(tokens[1].lexeme, "{{ value }}");
    Ok(())
}

#[test]
fn tokenize_unterminated_sequences() {
    let mut stream = TokenStream::new("\"unterminated");
    let err = stream.next().unwrap().unwrap_err();
    assert!(matches!(err, LayoutError::Parse { .. }));

    let mut stream = TokenStream::new("/* comment");
    let err = stream.next().unwrap().unwrap_err();
    assert!(matches!(err, LayoutError::Parse { .. }));
}
