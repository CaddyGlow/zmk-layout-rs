use std::cell::{Cell, RefCell};
use std::rc::Rc;

use mlua::{Error as LuaError, Lua, Result as LuaResult, Table as LuaTable, Value as LuaValue};

use crate::layout_engine::LayoutEngine;

pub type SharedLayout = Rc<RefCell<LayoutEngine>>;
pub type SharedLogs = Rc<RefCell<Vec<String>>>;

pub fn script_error(message: impl Into<String>) -> LuaError {
    LuaError::RuntimeError(message.into())
}

pub fn ensure_staged(applied: &Cell<bool>, context: &str) -> LuaResult<()> {
    if applied.get() {
        Err(script_error(format!(
            "{context} already applied; re-acquire builder to edit"
        )))
    } else {
        Ok(())
    }
}

pub fn require_positive_index(index: i64, context: &str) -> LuaResult<usize> {
    if index < 1 {
        return Err(script_error(format!(
            "{} index must be >= 1 (Lua 1-based)",
            context
        )));
    }
    Ok((index - 1) as usize)
}

pub fn lua_table_to_vec<'lua, T, F>(table: LuaTable<'lua>, mut convert: F) -> LuaResult<Vec<T>>
where
    F: FnMut(LuaValue<'lua>) -> LuaResult<T>,
{
    let mut result = Vec::new();
    for pair in table.sequence_values::<LuaValue>() {
        let value = pair?;
        result.push(convert(value)?);
    }
    Ok(result)
}

pub fn lua_table_to_strings(table: LuaTable<'_>) -> LuaResult<Vec<String>> {
    lua_table_to_vec(table, |value| match value {
        LuaValue::String(s) => Ok(s.to_str()?.to_string()),
        other => Err(script_error(format!(
            "expected string in array, got {}",
            other.type_name()
        ))),
    })
}

pub fn lua_table_to_u32(table: LuaTable<'_>) -> LuaResult<Vec<u32>> {
    lua_table_to_vec(table, |value| match value {
        LuaValue::Integer(num) => {
            if num < 1 {
                return Err(script_error("positions must be >= 1 (Lua 1-based)"));
            }
            Ok((num - 1) as u32)
        }
        LuaValue::Number(num) => {
            if num < 1.0 {
                return Err(script_error("positions must be >= 1 (Lua 1-based)"));
            }
            Ok((num as i64 - 1) as u32)
        }
        other => Err(script_error(format!(
            "expected number in array, got {}",
            other.type_name()
        ))),
    })
}

pub fn lua_value_to_optional_u32(value: LuaValue<'_>) -> LuaResult<Option<u32>> {
    match value {
        LuaValue::Nil => Ok(None),
        LuaValue::Integer(num) => {
            if num < 0 {
                Err(script_error("value must be non-negative"))
            } else {
                Ok(Some(num as u32))
            }
        }
        LuaValue::Number(num) => {
            if num < 0.0 {
                Err(script_error("value must be non-negative"))
            } else {
                Ok(Some(num as u32))
            }
        }
        other => Err(script_error(format!(
            "expected number or nil, got {}",
            other.type_name()
        ))),
    }
}

pub fn create_read_only_table<'lua>(
    lua: &'lua Lua,
    table: LuaTable<'lua>,
) -> LuaResult<LuaTable<'lua>> {
    let proxy = lua.create_table()?;
    let meta = lua.create_table()?;
    meta.set("__index", table)?;
    meta.set(
        "__newindex",
        lua.create_function(|_, _: (LuaValue, LuaValue)| {
            Err::<(), _>(script_error(
                "query results are read-only; use builders to mutate layout",
            ))
        })?,
    )?;
    proxy.set_metatable(Some(meta));
    Ok(proxy)
}
