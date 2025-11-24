use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use mlua::{
    Lua, Result as LuaResult, Table as LuaTable, UserData, UserDataMethods, Value as LuaValue,
};

use super::{
    behavior::BehaviorObject,
    combo::ComboObject,
    macro_builder::MacroObject,
    util::{
        SharedLayout, create_read_only_table, ensure_staged, require_positive_index, script_error,
    },
};

#[derive(Clone)]
pub struct LayerBuilder {
    name: String,
    layout: SharedLayout,
    staged: RefCell<HashMap<usize, String>>,
    replace_all: RefCell<Option<Vec<String>>>,
    metadata: RefCell<HashMap<String, String>>,
    applied: Cell<bool>,
}

impl LayerBuilder {
    pub fn new(name: String, layout: SharedLayout) -> Self {
        Self {
            name,
            layout,
            staged: RefCell::new(HashMap::new()),
            replace_all: RefCell::new(None),
            metadata: RefCell::new(HashMap::new()),
            applied: Cell::new(false),
        }
    }

    fn ensure_staged(&self) -> LuaResult<()> {
        ensure_staged(&self.applied, &format!("layer '{}'", self.name))
    }

    fn coerce_binding(&self, _: &Lua, value: LuaValue) -> LuaResult<String> {
        match value {
            LuaValue::String(s) => Ok(s.to_str()?.to_string()),
            LuaValue::UserData(data) => {
                if let Ok(combo) = data.borrow::<ComboObject>() {
                    return combo.as_binding_string();
                }
                if let Ok(mac) = data.borrow::<MacroObject>() {
                    return mac.as_binding_string();
                }
                if let Ok(behavior) = data.borrow::<BehaviorObject>() {
                    return behavior.as_binding_string();
                }
                Err(script_error("unsupported binding object type"))
            }
            other => Err(script_error(format!(
                "binding must be string or object, got {}",
                other.type_name()
            ))),
        }
    }

    fn apply_internal(&self) -> LuaResult<()> {
        self.ensure_staged()?;

        let mut engine = self.layout.borrow_mut();

        // Determine final bindings
        let (bindings, layer_exists) = if let Some(all) = self.replace_all.borrow_mut().take() {
            let normalized = engine
                .normalize_binding_list(&all)
                .map_err(|err| script_error(err.to_string()))?;
            let exists = engine.layer_names().contains(&self.name);
            (normalized, exists)
        } else {
            let mut current = engine
                .layer_bindings(&self.name)
                .map_err(|err| script_error(err.to_string()))?;
            for (index, binding) in self.staged.borrow().iter() {
                if *index >= current.len() {
                    return Err(script_error(format!(
                        "binding index {} out of range for layer '{}'",
                        index + 1,
                        self.name
                    )));
                }
                current[*index] = binding.clone();
            }
            let normalized = engine
                .normalize_binding_list(&current)
                .map_err(|err| script_error(err.to_string()))?;
            (normalized, true)
        };

        if layer_exists {
            engine
                .set_layer_bindings(&self.name, &bindings)
                .map_err(|err| script_error(err.to_string()))?;
        } else {
            engine
                .add_layer(&self.name, &bindings)
                .map_err(|err| script_error(err.to_string()))?;
        }

        if !self.metadata.borrow().is_empty() {
            let props: Vec<(String, String)> = self
                .metadata
                .borrow()
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            engine
                .set_layer_metadata(&self.name, &props)
                .map_err(|err| script_error(err.to_string()))?;
        }

        self.applied.set(true);
        Ok(())
    }
}

impl UserData for LayerBuilder {
    fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
        methods.add_method("bind", |lua, this, (index, binding): (i64, LuaValue)| {
            this.ensure_staged()?;
            let normalized_index = require_positive_index(index, "binding")?;
            let binding_text = this.coerce_binding(lua, binding)?;
            this.replace_all.borrow_mut().take(); // switch to partial mode
            this.staged
                .borrow_mut()
                .insert(normalized_index, binding_text);
            Ok(this.clone())
        });

        methods.add_method("bindings", |lua, this, table: LuaTable| {
            this.ensure_staged()?;
            let mut result = Vec::new();
            for value in table.sequence_values::<LuaValue>() {
                let value = value?;
                let binding_text = this.coerce_binding(lua, value)?;
                result.push(binding_text);
            }
            this.replace_all.borrow_mut().replace(result);
            this.staged.borrow_mut().clear();
            Ok(this.clone())
        });

        methods.add_method("meta", |_, this, (key, value): (String, LuaValue)| {
            this.ensure_staged()?;
            let rendered = match value {
                LuaValue::Boolean(flag) => flag.to_string(),
                LuaValue::Integer(num) => num.to_string(),
                LuaValue::Number(num) => num.to_string(),
                LuaValue::String(s) => s.to_str()?.to_string(),
                LuaValue::Nil => {
                    return Err(script_error("metadata values cannot be nil"));
                }
                other => {
                    return Err(script_error(format!(
                        "unsupported metadata type {}",
                        other.type_name()
                    )));
                }
            };
            this.metadata.borrow_mut().insert(key, rendered);
            Ok(this.clone())
        });

        methods.add_method("get_binding", |_, this, index: i64| {
            let normalized_index = require_positive_index(index, "binding")?;
            let engine = this.layout.borrow();
            let layer = engine
                .get_layer(&this.name)
                .ok_or_else(|| script_error(format!("layer '{}' not found", this.name)))?;
            let value = layer
                .bindings
                .get(normalized_index)
                .ok_or_else(|| script_error("binding index out of range"))?;
            Ok(value.clone())
        });

        methods.add_method("get_bindings", |lua, this, ()| {
            let engine = this.layout.borrow();
            let layer = engine
                .get_layer(&this.name)
                .ok_or_else(|| script_error(format!("layer '{}' not found", this.name)))?;
            let table = lua.create_table()?;
            for (idx, binding) in layer.bindings.iter().enumerate() {
                table.set(idx + 1, binding.clone())?;
            }
            create_read_only_table(lua, table)
        });

        methods.add_method("get_meta", |_, this, key: String| {
            let engine = this.layout.borrow();
            let value = engine
                .meta_to_string(&key)
                .unwrap_or_else(|| "".to_string());
            Ok(value)
        });

        methods.add_method("apply", |_, this, ()| this.apply());
    }
}

impl LayerBuilder {
    pub fn apply(&self) -> LuaResult<Self> {
        self.apply_internal()?;
        Ok(self.clone())
    }
}
