use zmk_layout_rs::bindings::{BindingParser, LayoutBinding, ParamValue};

fn parser() -> BindingParser {
    BindingParser::new()
}

#[test]
fn parse_simple_key() {
    let binding = parser().parse("&kp A");
    assert_eq!(binding.value, "&kp");
    assert_eq!(binding.params.len(), 1);
    assert_eq!(binding.params[0].value, ParamValue::Text("A".into()));
}

#[test]
fn parse_modifier_chain() {
    let binding = parser().parse("&kp LC(LS(A))");
    assert_eq!(binding.params[0].value, ParamValue::Text("LC".into()));
    assert_eq!(
        binding.params[0].params[0].value,
        ParamValue::Text("LS".into())
    );
}

#[test]
fn parse_hold_tap() {
    let binding = parser().parse("&mt LCTRL TAB");
    assert_eq!(binding.params.len(), 2);
    assert_eq!(binding.params[0].value, ParamValue::Text("LCTRL".into()));
    assert_eq!(binding.params[1].value, ParamValue::Text("TAB".into()));
}

#[test]
fn parse_numeric_layer() {
    let binding = parser().parse("&lt 1 SPACE");
    assert_eq!(binding.params[0].value, ParamValue::Integer(1));
    assert_eq!(binding.params[1].value, ParamValue::Text("SPACE".into()));
}

#[test]
fn parse_behavior_param() {
    let binding = parser().parse("&macro &kp LSHIFT");
    assert_eq!(binding.value, "&macro");
    assert_eq!(binding.params[0].value, ParamValue::Text("&kp".into()));
}

#[test]
fn parse_empty_defaults_to_none() {
    assert_eq!(parser().parse(""), LayoutBinding::none());
}

#[test]
fn parse_missing_ampersand_is_added() {
    let binding = parser().parse("kp A");
    assert_eq!(binding.value, "&kp");
}

#[test]
fn parse_bitwise_expression() {
    let binding = parser().parse("&test (10 & 0xFF) << 4");
    assert_eq!(binding.value, "&test");
    assert!(matches!(&binding.params[0].value, ParamValue::Text(expr) if expr.contains("<<")));
}

#[test]
fn flatten_behavior_rules() {
    let binding = parser().parse_with_behavior_rules("&mt LC RS");
    assert_eq!(binding.params.len(), 2);
    assert!(binding.params.iter().all(|param| param.params.is_empty()));
}

#[test]
fn modifier_chain_behavior_rules() {
    let binding = parser().parse_with_behavior_rules("&kp LC LS X");
    assert_eq!(binding.params.len(), 1);
    assert_eq!(binding.params[0].value, ParamValue::Text("LC".into()));
    assert_eq!(
        binding.params[0].params[0].value,
        ParamValue::Text("LS".into())
    );
}
