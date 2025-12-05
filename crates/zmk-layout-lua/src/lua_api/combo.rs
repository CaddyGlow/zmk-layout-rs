use mlua::{Result as LuaResult, Table as LuaTable, UserData, UserDataMethods, Value as LuaValue};
use std::cell::{Cell, RefCell};

use zmk_layout_core::layout_engine::LayerSelector;

use super::position::resolve_positions_1based;
use super::util::{
    ensure_staged, lua_table_to_strings, lua_value_to_optional_u32, script_error, SharedLayout,
    SharedPositions,
};

#[derive(Clone)]
pub struct ComboObject {
    name: String,
    layout: SharedLayout,
    positions: SharedPositions,
    keys: RefCell<Option<Vec<u32>>>,
    binding: RefCell<Option<String>>,
    timeout: RefCell<Option<Option<u32>>>,
    layers: RefCell<Vec<String>>,
    conditions: RefCell<Vec<String>>,
    applied: Cell<bool>,
}

impl ComboObject {
    pub fn new(name: String, layout: SharedLayout, positions: SharedPositions) -> Self {
        let combo = Self {
            name,
            layout,
            positions,
            keys: RefCell::new(None),
            binding: RefCell::new(None),
            timeout: RefCell::new(None),
            layers: RefCell::new(Vec::new()),
            conditions: RefCell::new(Vec::new()),
            applied: Cell::new(false),
        };
        combo.seed_from_layout();
        combo
    }

    pub fn as_binding_string(&self) -> LuaResult<String> {
        self.ensure_applied()?;
        match &*self.binding.borrow() {
            Some(binding) => Ok(binding.clone()),
            None => Err(script_error(format!(
                "combo '{}' requires a binding before use",
                self.name
            ))),
        }
    }

    fn apply_internal(&self) -> LuaResult<()> {
        self.ensure_staged()?;

        let keys = self
            .keys
            .borrow()
            .clone()
            .ok_or_else(|| script_error(format!("combo '{}' requires keys()", self.name)))?;
        let binding_value = self
            .binding
            .borrow()
            .clone()
            .ok_or_else(|| script_error(format!("combo '{}' requires binding()", self.name)))?;
        let timeout = self.timeout.borrow().clone().unwrap_or(None);
        let layers = self.layers.borrow().clone();
        let conditions = self.conditions.borrow().clone();

        let mut engine = self.layout.borrow_mut();
        let normalized = engine
            .normalize_binding(&binding_value)
            .map_err(|err| script_error(err.to_string()))?;
        let selectors: Vec<LayerSelector> = layers.into_iter().map(LayerSelector::Name).collect();
        let layer_indexes = engine
            .resolve_layer_selectors(&selectors)
            .map_err(|err| script_error(err.to_string()))?;
        engine
            .upsert_combo(
                &self.name,
                &normalized,
                &keys,
                timeout,
                &layer_indexes,
                &conditions,
            )
            .map_err(|err| script_error(err.to_string()))?;
        self.applied.set(true);
        Ok(())
    }

    fn seed_from_layout(&self) {
        let engine = self.layout.borrow();
        if let Some(state) = engine.combo_state(&self.name) {
            self.keys.borrow_mut().replace(state.key_positions);
            *self.binding.borrow_mut() = state.binding;
            *self.timeout.borrow_mut() = Some(state.timeout_ms);

            let layer_names = {
                let names = engine.layer_names();
                state
                    .layers
                    .iter()
                    .filter_map(|idx| names.get(*idx as usize).cloned())
                    .collect()
            };
            *self.layers.borrow_mut() = layer_names;
            *self.conditions.borrow_mut() = state.conditions;
        }
    }

    fn ensure_staged(&self) -> LuaResult<()> {
        ensure_staged(&self.applied, &format!("combo '{}'", self.name))
    }

    fn ensure_applied(&self) -> LuaResult<()> {
        if self.applied.get() {
            Ok(())
        } else {
            self.apply_internal()
        }
    }
}

impl UserData for ComboObject {
    fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
        methods.add_method("keys", |_, this, table: LuaTable| {
            this.ensure_staged()?;
            let positions = this.positions.borrow();
            let keys = resolve_positions_1based(&positions, table)?;
            *this.keys.borrow_mut() = Some(keys);
            Ok(this.clone())
        });

        methods.add_method("binding", |_, this, binding: String| {
            this.ensure_staged()?;
            *this.binding.borrow_mut() = Some(binding);
            Ok(this.clone())
        });

        methods.add_method("timeout", |_, this, value: LuaValue| {
            this.ensure_staged()?;
            let timeout = lua_value_to_optional_u32(value)?;
            *this.timeout.borrow_mut() = Some(timeout);
            Ok(this.clone())
        });

        methods.add_method("on_layers", |_, this, layers: LuaTable| {
            this.ensure_staged()?;
            let list = lua_table_to_strings(layers)?;
            *this.layers.borrow_mut() = list;
            Ok(this.clone())
        });

        methods.add_method("when", |_, this, condition: String| {
            this.ensure_staged()?;
            this.conditions.borrow_mut().push(condition);
            Ok(this.clone())
        });

        methods.add_method("get_keys", |lua, this, ()| {
            let table = lua.create_table()?;
            if let Some(keys) = this.keys.borrow().as_ref() {
                for (idx, key) in keys.iter().enumerate() {
                    table.set(idx + 1, *key + 1)?;
                }
            }
            Ok(table)
        });

        methods.add_method("get_binding", |_, this, ()| match &*this.binding.borrow() {
            Some(binding) => Ok(binding.clone()),
            None => Ok(String::new()),
        });

        methods.add_method("get_timeout", |_, this, ()| {
            Ok(this.timeout.borrow().clone().unwrap_or(None))
        });

        methods.add_method("apply", |_, this, ()| this.apply());
    }
}

impl ComboObject {
    pub fn apply(&self) -> LuaResult<Self> {
        self.apply_internal()?;
        Ok(self.clone())
    }
}
