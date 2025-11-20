use std::cell::{Cell, RefCell};

use mlua::{Result as LuaResult, UserData, UserDataMethods};

use super::util::{script_error, SharedLayout};

#[derive(Clone)]
pub struct InputObject {
    name: String,
    #[allow(dead_code)]
    layout: SharedLayout,
    input_type: RefCell<Option<String>>,
    cw: RefCell<Option<String>>,
    ccw: RefCell<Option<String>>,
    press: RefCell<Option<String>>,
    resolution: RefCell<Option<i64>>,
    applied: Cell<bool>,
}

impl InputObject {
    pub fn new(name: String, layout: SharedLayout) -> Self {
        Self {
            name,
            layout,
            input_type: RefCell::new(None),
            cw: RefCell::new(None),
            ccw: RefCell::new(None),
            press: RefCell::new(None),
            resolution: RefCell::new(None),
            applied: Cell::new(false),
        }
    }

    fn apply_internal(&self) -> LuaResult<()> {
        if self.applied.get() {
            return Ok(());
        }
        // No-op placeholder until encoder/sensor plumbing exists.
        self.applied.set(true);
        Ok(())
    }
}

impl UserData for InputObject {
    fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
        methods.add_method("type", |_, this, value: String| {
            this.input_type.borrow_mut().replace(value);
            Ok(this.clone())
        });
        methods.add_method("on_turn_cw", |_, this, binding: String| {
            this.cw.borrow_mut().replace(binding);
            Ok(this.clone())
        });
        methods.add_method("on_turn_ccw", |_, this, binding: String| {
            this.ccw.borrow_mut().replace(binding);
            Ok(this.clone())
        });
        methods.add_method("on_press", |_, this, binding: String| {
            this.press.borrow_mut().replace(binding);
            Ok(this.clone())
        });
        methods.add_method("resolution", |_, this, value: i64| {
            if value < 0 {
                return Err(script_error("resolution must be non-negative"));
            }
            this.resolution.borrow_mut().replace(value);
            Ok(this.clone())
        });
        methods.add_method("get_type", |_, this, ()| Ok(this.input_type.borrow().clone()));
        methods.add_method("get_cw_binding", |_, this, ()| Ok(this.cw.borrow().clone()));
        methods.add_method("get_ccw_binding", |_, this, ()| Ok(this.ccw.borrow().clone()));
        methods.add_method("name", |_, this, ()| Ok(this.name.clone()));
        methods.add_method("apply", |_, this, ()| this.apply());
    }
}

impl InputObject {
    pub fn apply(&self) -> LuaResult<Self> {
        self.apply_internal()?;
        Ok(self.clone())
    }
}
