//! Key position name support for Lua API.

use std::cell::RefCell;
use std::rc::Rc;

use mlua::{
    Result as LuaResult, Table as LuaTable, UserData, UserDataMethods, Value as LuaValue,
};

use zmk_layout_core::key_positions::KeyPositionMap;
use zmk_layout_core::profiles::KeyboardProfileDoc;

use super::util::script_error;

/// Thread-local shared position map.
pub type SharedPositions = Rc<RefCell<KeyPositionMap>>;

/// Lua wrapper for KeyPositionMap providing key position name lookups.
#[derive(Clone)]
pub struct PositionMapObject {
    inner: SharedPositions,
}

impl PositionMapObject {
    pub fn new(map: KeyPositionMap) -> Self {
        Self {
            inner: Rc::new(RefCell::new(map)),
        }
    }

    pub fn from_shared(positions: SharedPositions) -> Self {
        Self { inner: positions }
    }

    pub fn shared(&self) -> SharedPositions {
        Rc::clone(&self.inner)
    }
}

impl UserData for PositionMapObject {
    fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
        // Get numeric index for a position name
        methods.add_method("get", |_, this, name: String| {
            Ok(this.inner.borrow().get(&name))
        });

        // Get position name for a numeric index
        methods.add_method("get_name", |_, this, index: u32| {
            Ok(this.inner.borrow().get_name(index).map(String::from))
        });

        // Check if position name exists
        methods.add_method("contains", |_, this, name: String| {
            Ok(this.inner.borrow().contains(&name))
        });

        // Get all position names
        methods.add_method("names", |lua, this, ()| {
            let names: Vec<String> = this
                .inner
                .borrow()
                .names()
                .into_iter()
                .map(String::from)
                .collect();
            lua.create_sequence_from(names)
        });

        // Resolve mixed positions to indices
        methods.add_method("resolve", |lua, this, table: LuaTable| {
            let map = this.inner.borrow();
            let result = resolve_positions_0based(&map, table)?;
            lua.create_sequence_from(result)
        });
    }
}

/// Load a PositionMapObject from a keyboard profile name.
pub fn load_positions(profile_name: &str) -> LuaResult<PositionMapObject> {
    let profile = KeyboardProfileDoc::load(profile_name)
        .map_err(|err| script_error(format!("failed to load profile '{}': {}", profile_name, err)))?;
    let map = KeyPositionMap::from_profile(&profile);
    Ok(PositionMapObject::new(map))
}

/// Resolve a single Lua value to a position index (0-based output).
fn resolve_single_position_0based(map: &KeyPositionMap, value: LuaValue) -> LuaResult<u32> {
    match value {
        LuaValue::Integer(num) => {
            if num < 0 {
                return Err(script_error("position index cannot be negative"));
            }
            Ok(num as u32)
        }
        LuaValue::Number(num) => {
            if num < 0.0 {
                return Err(script_error("position index cannot be negative"));
            }
            Ok(num as u32)
        }
        LuaValue::String(s) => {
            let s = s.to_str()?;
            // Try parsing as number
            if let Ok(num) = s.parse::<u32>() {
                return Ok(num);
            }
            // Try as position name
            if let Some(index) = map.get(s) {
                return Ok(index);
            }
            Err(script_error(format!("unknown position: {}", s)))
        }
        other => Err(script_error(format!(
            "position must be number or string, got {}",
            other.type_name()
        ))),
    }
}

/// Resolve a Lua table of positions to indices (0-based output).
fn resolve_positions_0based(map: &KeyPositionMap, table: LuaTable) -> LuaResult<Vec<u32>> {
    let mut result = Vec::new();
    for value in table.sequence_values::<LuaValue>() {
        let index = resolve_single_position_0based(map, value?)?;
        result.push(index);
    }
    Ok(result)
}

/// Resolve a Lua table of positions using 1-based indexing for pure numbers.
///
/// - Integers/Numbers: treated as 1-based, converted to 0-based
/// - Position name strings: map directly to 0-based indices
pub fn resolve_positions_1based(map: &KeyPositionMap, table: LuaTable) -> LuaResult<Vec<u32>> {
    let mut result = Vec::new();

    for value in table.sequence_values::<LuaValue>() {
        let value = value?;
        let index = match value {
            LuaValue::Integer(num) => {
                if num < 1 {
                    return Err(script_error("positions must be >= 1 (Lua 1-based)"));
                }
                (num - 1) as u32
            }
            LuaValue::Number(num) => {
                if num < 1.0 {
                    return Err(script_error("positions must be >= 1 (Lua 1-based)"));
                }
                (num as i64 - 1) as u32
            }
            LuaValue::String(s) => {
                let s = s.to_str()?;
                // Try parsing as number (1-based)
                if let Ok(num) = s.parse::<i64>() {
                    if num < 1 {
                        return Err(script_error("positions must be >= 1 (Lua 1-based)"));
                    }
                    (num - 1) as u32
                } else if let Some(index) = map.get(s) {
                    // Position name maps directly to 0-based
                    index
                } else {
                    return Err(script_error(format!("unknown position: {}", s)));
                }
            }
            other => {
                return Err(script_error(format!(
                    "position must be number or string, got {}",
                    other.type_name()
                )));
            }
        };
        result.push(index);
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_glove80_positions() {
        let map = load_positions("glove80").unwrap();
        assert_eq!(map.inner.borrow().get("LH_C6R1"), Some(0));
        assert_eq!(map.inner.borrow().get("LH_T1"), Some(52));
    }
}
