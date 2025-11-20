use std::cell::{Cell, RefCell};

use mlua::{Result as LuaResult, UserData, UserDataMethods, Value as LuaValue};

use super::util::{create_read_only_table, script_error, SharedLayout};

#[derive(Clone)]
pub struct MacroObject {
    name: String,
    #[allow(dead_code)]
    layout: SharedLayout,
    actions: RefCell<Vec<String>>,
    applied: Cell<bool>,
}

impl MacroObject {
    pub fn new(name: String, layout: SharedLayout) -> Self {
        Self {
            name,
            layout,
            actions: RefCell::new(Vec::new()),
            applied: Cell::new(false),
        }
    }

    fn push_action(&self, action: String) {
        self.actions.borrow_mut().push(action);
    }

    pub fn as_binding_string(&self) -> LuaResult<String> {
        self.apply_internal()?;
        Ok(format!("&{}", self.name))
    }

    fn apply_internal(&self) -> LuaResult<()> {
        if self.applied.get() {
            return Ok(());
        }
        // Macro application is a no-op for now; assumes underlying DTS already defines behavior.
        self.applied.set(true);
        Ok(())
    }
}

impl UserData for MacroObject {
    fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
        methods.add_method("press", |_, this, keys: LuaValue| {
            this.push_action(format!("press:{:?}", keys));
            Ok(this.clone())
        });
        methods.add_method("release", |_, this, keys: LuaValue| {
            this.push_action(format!("release:{:?}", keys));
            Ok(this.clone())
        });
        methods.add_method("tap", |_, this, keys: LuaValue| {
            this.push_action(format!("tap:{:?}", keys));
            Ok(this.clone())
        });
        methods.add_method("wait", |_, this, ms: i64| {
            if ms < 0 {
                return Err(script_error("wait duration must be non-negative"));
            }
            this.push_action(format!("wait:{ms}"));
            Ok(this.clone())
        });
        methods.add_method("wait_release", |_, this, ()| {
            this.push_action("wait_release".into());
            Ok(this.clone())
        });
        methods.add_method("wait_tap", |_, this, ()| {
            this.push_action("wait_tap".into());
            Ok(this.clone())
        });
        methods.add_method("get_actions", |lua, this, ()| {
            let table = lua.create_table()?;
            for (idx, action) in this.actions.borrow().iter().enumerate() {
                table.set(idx + 1, action.clone())?;
            }
            create_read_only_table(lua, table)
        });
        methods.add_method("apply", |_, this, ()| this.apply());
    }
}

impl MacroObject {
    pub fn apply(&self) -> LuaResult<Self> {
        self.apply_internal()?;
        Ok(self.clone())
    }
}
