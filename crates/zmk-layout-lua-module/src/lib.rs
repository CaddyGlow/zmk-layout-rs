//! Native Lua module for ZMK layout manipulation.
//!
//! This crate builds a shared library (`.so`/`.dll`/`.dylib`) that can be
//! loaded by any Lua 5.4 interpreter using `require("zmk_layout")`.
//!
//! ## Usage from Lua
//!
//! ```lua
//! local zmk = require("zmk_layout")
//!
//! -- Create a new layout
//! local layout = zmk.layout()
//!
//! -- Load from file
//! layout:load_dts("path/to/keymap.keymap")
//!
//! -- Or parse inline
//! layout:parse_dts([[
//!     / {
//!         keymap {
//!             compatible = "zmk,keymap";
//!             default_layer { bindings = <&kp A>; };
//!         };
//!     };
//! ]])
//!
//! -- Modify layers
//! layout:layer("default"):bindings({"&kp A", "&kp B"}):apply()
//!
//! -- Add combos
//! layout:combo("esc"):keys({1, 2}):binding("&kp ESC"):apply()
//!
//! -- Export
//! layout:save_dts("output.keymap")
//! local dts_string = layout:to_dts_string()
//! ```

use mlua::prelude::*;

/// Entry point for the Lua module.
/// Called when Lua executes `require("zmk_layout")`.
#[mlua::lua_module]
fn zmk_layout(lua: &Lua) -> LuaResult<LuaTable> {
    let exports = lua.create_table()?;

    // Create a layout constructor function
    let layout_fn = lua.create_function(|lua, ()| {
        create_layout_object(lua)
    })?;
    exports.set("layout", layout_fn)?;

    // Add version info
    exports.set("version", env!("CARGO_PKG_VERSION"))?;

    // Add a simple test function
    let hello_fn = lua.create_function(|_, name: Option<String>| {
        let name = name.unwrap_or_else(|| "world".to_string());
        Ok(format!("Hello, {}! ZMK Layout module is working.", name))
    })?;
    exports.set("hello", hello_fn)?;

    Ok(exports)
}

/// Create a new layout object with the fluent API
fn create_layout_object(lua: &Lua) -> LuaResult<LuaAnyUserData> {
    use std::cell::RefCell;
    use std::rc::Rc;
    use zmk_layout_core::layout_engine::LayoutEngine;

    // Create empty engine
    let engine = LayoutEngine::empty();
    let shared_layout = Rc::new(RefCell::new(engine));
    let shared_logs: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));

    // Create and return the layout API object
    zmk_layout_lua::create_layout_api(lua, shared_layout, shared_logs)
}
