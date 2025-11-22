use crate::adapters::{
    AdapterError, RegexExtractionConfig, export_standard_str_with_regex_extractions,
};

/// Build the default MoErgo regex extractors that mirror the comment markers in the Glove80 template.
pub fn default_extraction_config() -> Vec<RegexExtractionConfig> {
    vec![
        RegexExtractionConfig::new(
            "custom_devicetree",
            r"/\*\s*Custom\s+Device-tree\s*\*/",
            r"/\*\s*Input\s+Listeners\s*\*/",
        )
        .expect("hardcoded regex should compile")
        .with_strip_for_parse(true),
        RegexExtractionConfig::new(
            "input_listeners_dtsi",
            r"/\*\s*Input\s+Listeners\s*\*/",
            r"/\*\s*System\s+behavior\s+and\s+Macros\s*\*/",
        )
        .expect("hardcoded regex should compile"),
        RegexExtractionConfig::new(
            "system_behaviors_dts",
            r"/\*\s*System\s+behavior\s+and\s+Macros\s*\*/",
            r"/\*\s*(?:#define\s+for\s+key\s+positions|Custom\s+Defined\s+Behaviors|Automatically\s+generated\s+macro|Automatically\s+generated\s+behavior|Automatically\s+generated\s+combos|Automatically\s+generated\s+keymap|$)",
        )
        .expect("hardcoded regex should compile")
        .with_strip_for_parse(true),
        RegexExtractionConfig::new(
            "custom_defined_behaviors",
            r"/\*\s*Custom\s+Defined\s+Behaviors\s*\*/",
            r"/\*\s*(?:Automatically\s+generated\s+macro|Automatically\s+generated\s+behavior|Automatically\s+generated\s+combos|Automatically\s+generated\s+keymap|$)",
        )
        .expect("hardcoded regex should compile")
        .with_strip_for_parse(true),
        RegexExtractionConfig::new(
            "user_macros_dtsi",
            r"/\*\s*Automatically\s+generated\s+macro\s+definitions\s*\*/",
            r"/\*\s*(?:Automatically\s+generated\s+behavior|Automatically\s+generated\s+combos|Automatically\s+generated\s+keymap|$)",
        )
        .expect("hardcoded regex should compile"),
        RegexExtractionConfig::new(
            "user_behaviors_dtsi",
            r"/\*\s*Automatically\s+generated\s+behavior\s+definitions\s*\*/",
            r"/\*\s*(?:Automatically\s+generated\s+combos|Automatically\s+generated\s+keymap|$)",
        )
        .expect("hardcoded regex should compile"),
        RegexExtractionConfig::new(
            "combos_dtsi",
            r"/\*\s*Automatically\s+generated\s+combos\s+definitions\s*\*/",
            r"/\*\s*(?:Automatically\s+generated\s+keymap|$)",
        )
        .expect("hardcoded regex should compile"),
        RegexExtractionConfig::new(
            "keymap_node",
            r"/\*\s*Automatically\s+generated\s+keymap\s*\*/",
            r"\z",
        )
        .expect("hardcoded regex should compile"),
    ]
}

/// Export a MoErgo-rendered DTSI into the standard JSON layout using regex-based section capture.
pub fn export_standard_str_from_moergo_dtsi(rendered_source: &str) -> Result<String, AdapterError> {
    export_standard_str_with_regex_extractions(rendered_source, &default_extraction_config())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::AdapterLayout;

    const SAMPLE_DTS: &str = r#"/* Custom Device-tree */
&sensor {
    status = "okay";
};

/* Input Listeners */
&mmv_input_listener {
    input-processors = <&zip_xy_scaler 4 5>;
};

/* System behavior and Macros */
#define SYS_MACRO 1

/* Custom Defined Behaviors */
/ {
    behaviors {
        vendor_magic: vendor_magic {
            compatible = "zmk,behavior-macro";
            #binding-cells = <0>;
            bindings = < &kp A >;
        };
    };
};

/* Automatically generated macro definitions */
/ {
    macros {
        shout: shout {
            compatible = "zmk,behavior-macro";
            #binding-cells = <0>;
            bindings = < &kp A >;
        };
    };
};

/* Automatically generated behavior definitions */
/ {
    behaviors {
        tap_dance: tap_dance {
            compatible = "zmk,behavior-tap-dance";
            #binding-cells = <2>;
            bindings = < &kp X &kp Y >;
        };
    };
};

/* Automatically generated combos definitions */
/ {
    combos {
        compatible = "zmk,combos";
        combo_esc {
            key-positions = <0 1>;
            bindings = < &kp ESC >;
        };
    };
};

/* Automatically generated keymap */
/ {
    keymap {
        compatible = "zmk,keymap";
        base {
            bindings = < &kp A &kp B &kp C >;
        };
    };
};
"#;

    #[test]
    fn regex_extraction_captures_user_sections() {
        let json =
            export_standard_str_from_moergo_dtsi(SAMPLE_DTS).expect("regex-based export succeeds");
        let layout =
            AdapterLayout::from_standard_json(&json).expect("exported JSON parses back to layout");

        assert_eq!(layout.layers.len(), 1, "keymap should be preserved");
        assert_eq!(layout.combos.len(), 1, "combos parsed");
        assert_eq!(layout.behaviors.len(), 1, "behaviors parsed");
        assert_eq!(layout.macros.len(), 1, "macros parsed");
        assert_eq!(
            layout.input_listeners.len(),
            1,
            "input listener section should remain available for parsing"
        );

        let extras = &layout.metadata.extras;
        let custom_dt = extras
            .get("custom_devicetree")
            .and_then(|v| v.as_str())
            .expect("custom devicetree captured");
        assert!(custom_dt.contains("&sensor"));

        let system = extras
            .get("system_behaviors_dts")
            .and_then(|v| v.as_str())
            .expect("system behaviors captured");
        assert!(system.contains("SYS_MACRO"));

        let listeners = extras
            .get("input_listeners_dtsi")
            .and_then(|v| v.as_str())
            .expect("input listeners captured");
        assert!(listeners.contains("&mmv_input_listener"));
    }
}
