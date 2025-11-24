use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;

use mlua::{
    Lua, Result as LuaResult, Table as LuaTable, UserData, UserDataMethods, Value as LuaValue,
};
use toml::Value as TomlValue;

use crate::adapters::BehaviorSpec;

use super::util::{
    SharedLayout, create_read_only_table, ensure_staged, lua_table_to_strings, script_error,
};

#[derive(Clone)]
pub struct BehaviorObject {
    name: String,
    layout: SharedLayout,
    params: RefCell<BTreeMap<String, TomlValue>>,
    bindings: RefCell<Option<Vec<String>>>,
    applied: Cell<bool>,
}

impl BehaviorObject {
    pub fn new(name: String, layout: SharedLayout) -> Self {
        let behavior = Self {
            name,
            layout,
            params: RefCell::new(BTreeMap::new()),
            bindings: RefCell::new(None),
            applied: Cell::new(false),
        };
        behavior.seed_from_layout();
        behavior
    }

    pub fn as_binding_string(&self) -> LuaResult<String> {
        self.ensure_applied()?;
        Ok(format!("&{}", self.name))
    }

    fn apply_internal(&self) -> LuaResult<()> {
        self.ensure_staged()?;
        let mut metadata = self.params.borrow().clone();
        if let Some(bindings) = self.bindings.borrow().clone() {
            let boxed = bindings
                .into_iter()
                .map(TomlValue::String)
                .collect::<Vec<_>>();
            metadata.insert("bindings".to_string(), TomlValue::Array(boxed));
        }
        let mut engine = self.layout.borrow_mut();
        engine
            .set_behavior_settings(&self.name, &metadata)
            .map_err(|err| script_error(err.to_string()))?;
        self.applied.set(true);
        Ok(())
    }

    fn behavior_definition(&self) -> Option<BehaviorSpec> {
        let engine = self.layout.borrow();
        engine
            .document()
            .behaviors
            .iter()
            .find(|behavior| behavior.name == self.name)
            .cloned()
    }

    fn seed_from_layout(&self) {
        if let Some(def) = self.behavior_definition() {
            if !def.bindings.is_empty() {
                self.bindings.borrow_mut().replace(def.bindings);
            }
        }
    }

    fn ensure_staged(&self) -> LuaResult<()> {
        ensure_staged(&self.applied, &format!("behavior '{}'", self.name))
    }

    fn ensure_applied(&self) -> LuaResult<()> {
        if self.applied.get() {
            Ok(())
        } else {
            self.apply_internal()
        }
    }
}

impl UserData for BehaviorObject {
    fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
        methods.add_method("param", |_, this, (key, value): (String, LuaValue)| {
            this.ensure_staged()?;
            let toml = lua_value_to_toml(value)?;
            this.params.borrow_mut().insert(key, toml);
            Ok(this.clone())
        });

        methods.add_method("bindings", |_, this, bindings: LuaTable| {
            this.ensure_staged()?;
            let list = lua_table_to_strings(bindings)?;
            this.bindings.borrow_mut().replace(list);
            Ok(this.clone())
        });

        methods.add_method("get_param", |lua, this, key: String| {
            if let Some(value) = this.params.borrow().get(&key) {
                return toml_to_lua_value(lua, value);
            }
            if let Some(def) = this.behavior_definition() {
                if let Some(value) = def.properties.get(&key) {
                    return Ok(LuaValue::String(lua.create_string(value)?));
                }
            }
            Ok(LuaValue::Nil)
        });

        methods.add_method("get_bindings", |lua, this, ()| {
            let table = lua.create_table()?;
            if let Some(bindings) = this.bindings.borrow().as_ref() {
                for (idx, binding) in bindings.iter().enumerate() {
                    table.set(idx + 1, binding.clone())?;
                }
            } else if let Some(def) = this.behavior_definition() {
                for (idx, binding) in def.bindings.iter().enumerate() {
                    table.set(idx + 1, binding.clone())?;
                }
            }
            create_read_only_table(lua, table)
        });

        methods.add_method("apply", |_, this, ()| this.apply());
    }
}

impl BehaviorObject {
    pub fn apply(&self) -> LuaResult<Self> {
        self.apply_internal()?;
        Ok(self.clone())
    }
}

fn lua_value_to_toml(value: LuaValue<'_>) -> LuaResult<TomlValue> {
    match value {
        LuaValue::Nil => Err(script_error("parameter values cannot be nil")),
        LuaValue::Boolean(flag) => Ok(TomlValue::Boolean(flag)),
        LuaValue::Integer(num) => Ok(TomlValue::Integer(num)),
        LuaValue::Number(num) => Ok(TomlValue::Float(num)),
        LuaValue::String(s) => Ok(TomlValue::String(s.to_str()?.to_string())),
        LuaValue::Table(table) => {
            let is_array = table.contains_key(1)?;
            if is_array {
                let mut items = Vec::new();
                for pair in table.sequence_values::<LuaValue>() {
                    let value = pair?;
                    items.push(lua_value_to_toml(value)?);
                }
                Ok(TomlValue::Array(items))
            } else {
                let mut map = toml::map::Map::new();
                for pair in table.pairs::<LuaValue, LuaValue>() {
                    let (key, value) = pair?;
                    let key = match key {
                        LuaValue::String(s) => s.to_str()?.to_string(),
                        other => {
                            return Err(script_error(format!(
                                "table keys must be strings, got {}",
                                other.type_name()
                            )));
                        }
                    };
                    map.insert(key, lua_value_to_toml(value)?);
                }
                Ok(TomlValue::Table(map))
            }
        }
        other => Err(script_error(format!(
            "unsupported value type {} for parameter",
            other.type_name()
        ))),
    }
}

fn toml_to_lua_value<'lua>(lua: &'lua Lua, value: &TomlValue) -> LuaResult<LuaValue<'lua>> {
    Ok(match value {
        TomlValue::String(text) => LuaValue::String(lua.create_string(text)?),
        TomlValue::Integer(num) => LuaValue::Integer(*num),
        TomlValue::Float(num) => LuaValue::Number(*num),
        TomlValue::Boolean(flag) => LuaValue::Boolean(*flag),
        TomlValue::Array(items) => {
            let table = lua.create_table()?;
            for (idx, entry) in items.iter().enumerate() {
                table.set(idx + 1, toml_to_lua_value(lua, entry)?)?;
            }
            LuaValue::Table(table)
        }
        TomlValue::Table(entries) => {
            let table = lua.create_table()?;
            for (key, entry) in entries {
                table.set(key.as_str(), toml_to_lua_value(lua, entry)?)?;
            }
            LuaValue::Table(table)
        }
        TomlValue::Datetime(dt) => LuaValue::String(lua.create_string(&dt.to_string())?),
    })
}
