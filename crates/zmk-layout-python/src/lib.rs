//! Python bindings for ZMK layout manipulation.
//!
//! This crate provides a Python extension module that can be imported using
//! `import zmk_layout` after installation.
//!
//! ## Usage from Python
//!
//! ```python
//! import zmk_layout
//!
//! # Create a new layout
//! layout = zmk_layout.Layout()
//!
//! # Load from file
//! layout.load_dts("path/to/keymap.keymap")
//!
//! # Or parse inline
//! layout.parse_dts('''
//!     / {
//!         keymap {
//!             compatible = "zmk,keymap";
//!             default_layer { bindings = <&kp A>; };
//!         };
//!     };
//! ''')
//!
//! # Modify layers using fluent API
//! layout.layer("default").bindings(["&kp A", "&kp B"]).apply()
//!
//! # Add combos
//! layout.combo("esc").keys([1, 2]).binding("&kp ESC").apply()
//!
//! # Export
//! layout.save_dts("output.keymap")
//! dts_string = layout.to_dts_string()
//! ```

mod python_api;

use pyo3::prelude::*;

/// A simple test function.
#[pyfunction]
#[pyo3(signature = (name=None))]
fn hello(name: Option<String>) -> String {
    let name = name.unwrap_or_else(|| "world".to_string());
    format!("Hello, {}! ZMK Layout module is working.", name)
}

/// Entry point for the Python module.
/// Called when Python executes `import zmk_layout`.
#[pymodule]
fn zmk_layout(m: &Bound<'_, PyModule>) -> PyResult<()> {
    // Add the Layout class
    m.add_class::<python_api::Layout>()?;

    // Add builder classes
    m.add_class::<python_api::LayerBuilder>()?;
    m.add_class::<python_api::ComboObject>()?;
    m.add_class::<python_api::BehaviorObject>()?;
    m.add_class::<python_api::MacroObject>()?;
    m.add_class::<python_api::InputObject>()?;
    m.add_class::<python_api::ConditionalObject>()?;

    // Add query classes
    m.add_class::<python_api::LayerInfo>()?;
    m.add_class::<python_api::ComboInfo>()?;
    m.add_class::<python_api::BehaviorInfo>()?;

    // Add version info
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;

    // Add the hello function
    m.add_function(wrap_pyfunction!(hello, m)?)?;

    Ok(())
}
