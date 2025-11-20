use std::rc::Rc;

use mlua::{Lua, Result as LuaResult, UserData, UserDataMethods};

use super::{
    behavior::BehaviorObject,
    combo::ComboObject,
    conditional::ConditionalObject,
    input::InputObject,
    layer::LayerBuilder,
    macro_builder::MacroObject,
    query::{
        BehaviorInfoObject, ComboInfoObject, LayerInfoObject, list_behavior_definitions,
        list_combo_definitions,
    },
    util::{SharedLayout, SharedLogs},
};

#[derive(Clone)]
pub struct LayoutApi {
    layout: SharedLayout,
    #[allow(dead_code)]
    logs: SharedLogs,
}

impl LayoutApi {
    pub fn new(layout: SharedLayout, logs: SharedLogs) -> Self {
        Self { layout, logs }
    }
}

impl UserData for LayoutApi {
    fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
        methods.add_method("layer", |_, this, name: String| {
            Ok(LayerBuilder::new(name, Rc::clone(&this.layout)))
        });
        methods.add_method("combo", |_, this, name: String| {
            Ok(ComboObject::new(name, Rc::clone(&this.layout)))
        });
        methods.add_method("behavior", |_, this, name: String| {
            Ok(BehaviorObject::new(name, Rc::clone(&this.layout)))
        });
        methods.add_method("macro", |_, this, name: String| {
            Ok(MacroObject::new(name, Rc::clone(&this.layout)))
        });
        methods.add_method("input", |_, this, name: String| {
            Ok(InputObject::new(name, Rc::clone(&this.layout)))
        });
        methods.add_method("conditional", |_, this, name: String| {
            Ok(ConditionalObject::new(name, Rc::clone(&this.layout)))
        });

        methods.add_method("get_layer", |_, this, name: String| {
            let engine = this.layout.borrow();
            match LayerInfoObject::from_engine(&engine, &name)? {
                Some(info) => Ok(Some(info)),
                None => Ok(None),
            }
        });

        methods.add_method("get_combo", |_, this, name: String| {
            let engine = this.layout.borrow();
            let def = list_combo_definitions(&engine)
                .into_iter()
                .find(|combo| combo.name == name);
            Ok(def.map(ComboInfoObject::from_definition))
        });

        methods.add_method("get_behavior", |_, this, name: String| {
            let engine = this.layout.borrow();
            let def = list_behavior_definitions(&engine)
                .into_iter()
                .find(|behavior| behavior.name == name);
            Ok(def.map(BehaviorInfoObject::from_definition))
        });

        methods.add_method("list_layers", |lua, this, ()| {
            let engine = this.layout.borrow();
            let names = engine.layer_names();
            lua.create_sequence_from(names)
        });

        methods.add_method("list_combos", |lua, this, ()| {
            let engine = this.layout.borrow();
            let names = list_combo_definitions(&engine)
                .into_iter()
                .map(|combo| combo.name)
                .collect::<Vec<_>>();
            lua.create_sequence_from(names)
        });

        methods.add_method("list_behaviors", |lua, this, ()| {
            let engine = this.layout.borrow();
            let names = list_behavior_definitions(&engine)
                .into_iter()
                .map(|behavior| behavior.name)
                .collect::<Vec<_>>();
            lua.create_sequence_from(names)
        });
    }
}

pub fn install_layout_api(lua: &Lua, layout: SharedLayout, logs: SharedLogs) -> LuaResult<()> {
    let api = LayoutApi::new(layout, logs);
    lua.globals().set("layout", api)?;
    Ok(())
}
