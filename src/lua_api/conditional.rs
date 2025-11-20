use std::cell::{Cell, RefCell};

use mlua::{Result as LuaResult, UserData, UserDataMethods};

use super::util::SharedLayout;

#[derive(Clone)]
pub struct ConditionalObject {
    name: String,
    #[allow(dead_code)]
    layout: SharedLayout,
    condition: RefCell<Option<String>>,
    then_layer: RefCell<Option<String>>,
    else_layer: RefCell<Option<String>>,
    applied: Cell<bool>,
}

impl ConditionalObject {
    pub fn new(name: String, layout: SharedLayout) -> Self {
        Self {
            name,
            layout,
            condition: RefCell::new(None),
            then_layer: RefCell::new(None),
            else_layer: RefCell::new(None),
            applied: Cell::new(false),
        }
    }

    fn apply_internal(&self) -> LuaResult<()> {
        if self.applied.get() {
            return Ok(());
        }
        // No-op placeholder until conditional support is wired into the engine.
        self.applied.set(true);
        Ok(())
    }
}

impl UserData for ConditionalObject {
    fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
        methods.add_method("condition", |_, this, expr: String| {
            this.condition.borrow_mut().replace(expr);
            Ok(this.clone())
        });
        methods.add_method("then_layer", |_, this, layer: String| {
            this.then_layer.borrow_mut().replace(layer);
            Ok(this.clone())
        });
        methods.add_method("else_layer", |_, this, layer: String| {
            this.else_layer.borrow_mut().replace(layer);
            Ok(this.clone())
        });
        methods.add_method("get_condition", |_, this, ()| Ok(this.condition.borrow().clone()));
        methods.add_method("get_then_layer", |_, this, ()| Ok(this.then_layer.borrow().clone()));
        methods.add_method("get_else_layer", |_, this, ()| Ok(this.else_layer.borrow().clone()));
        methods.add_method("name", |_, this, ()| Ok(this.name.clone()));
        methods.add_method("apply", |_, this, ()| this.apply());
    }
}

impl ConditionalObject {
    pub fn apply(&self) -> LuaResult<Self> {
        self.apply_internal()?;
        Ok(self.clone())
    }
}
