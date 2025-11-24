use std::collections::BTreeMap;

use mlua::{Result as LuaResult, UserData, UserDataMethods};

use crate::adapters::{BehaviorSpec, ComboSpec};
use crate::layout_engine::LayoutEngine;

use super::util::create_read_only_table;

#[derive(Clone)]
pub struct LayerInfoObject {
    name: String,
    bindings: Vec<String>,
    metadata: BTreeMap<String, String>,
}

#[derive(Clone)]
pub struct ComboInfoObject {
    name: String,
    keys: Vec<u32>,
    binding: Option<String>,
    timeout: Option<u32>,
}

#[derive(Clone)]
pub struct BehaviorInfoObject {
    name: String,
    bindings: Vec<String>,
    properties: BTreeMap<String, String>,
}

impl LayerInfoObject {
    pub fn from_engine(engine: &LayoutEngine, name: &str) -> LuaResult<Option<Self>> {
        if let Some(info) = engine.get_layer(name) {
            Ok(Some(Self {
                name: info.name,
                bindings: info.bindings,
                metadata: BTreeMap::new(),
            }))
        } else {
            Ok(None)
        }
    }
}

impl UserData for LayerInfoObject {
    fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
        methods.add_method("name", |_, this, ()| Ok(this.name.clone()));
        methods.add_method("bindings", |lua, this, ()| {
            let table = lua.create_table()?;
            for (idx, binding) in this.bindings.iter().enumerate() {
                table.set(idx + 1, binding.clone())?;
            }
            create_read_only_table(lua, table)
        });
        methods.add_method("metadata", |lua, this, ()| {
            let table = lua.create_table()?;
            for (key, value) in &this.metadata {
                table.set(key.as_str(), value.as_str())?;
            }
            create_read_only_table(lua, table)
        });
    }
}

impl ComboInfoObject {
    pub fn from_definition(def: ComboSpec) -> Self {
        let binding = def.binding.clone();
        Self {
            name: def.name,
            keys: def.key_positions,
            binding,
            timeout: def.timeout_ms,
        }
    }
}

impl UserData for ComboInfoObject {
    fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
        methods.add_method("keys", |lua, this, ()| {
            let table = lua.create_table()?;
            for (idx, key) in this.keys.iter().enumerate() {
                table.set(idx + 1, *key + 1)?;
            }
            create_read_only_table(lua, table)
        });
        methods.add_method("binding", |_, this, ()| Ok(this.binding.clone()));
        methods.add_method("timeout", |_, this, ()| Ok(this.timeout));
        methods.add_method("name", |_, this, ()| Ok(this.name.clone()));
    }
}

impl BehaviorInfoObject {
    pub fn from_definition(def: BehaviorSpec) -> Self {
        let properties = def.properties.clone();
        Self {
            name: def.name,
            bindings: def.bindings,
            properties,
        }
    }
}

impl UserData for BehaviorInfoObject {
    fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
        methods.add_method("bindings", |lua, this, ()| {
            let table = lua.create_table()?;
            for (idx, binding) in this.bindings.iter().enumerate() {
                table.set(idx + 1, binding.clone())?;
            }
            create_read_only_table(lua, table)
        });
        methods.add_method("get", |_, this, key: String| {
            Ok(this.properties.get(&key).cloned())
        });
        methods.add_method("name", |_, this, ()| Ok(this.name.clone()));
    }
}

pub fn list_combo_definitions(engine: &LayoutEngine) -> Vec<ComboSpec> {
    engine.document().combos.clone()
}

pub fn list_behavior_definitions(engine: &LayoutEngine) -> Vec<BehaviorSpec> {
    engine.document().behaviors.clone()
}
