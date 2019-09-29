use std::sync::Arc;

use rlua::{
    AnyUserData, ExternalError, Function, MetaMethod, Result, String, UserData, UserDataMethods,
};

include!("_lua.rs");

#[test]
fn test_user_data() -> Result<()> {
    struct UserData1(i64);
    struct UserData2(Box<i64>);

    impl UserData for UserData1 {};
    impl UserData for UserData2 {};

    let lua = make_lua();
    let userdata1 = lua.create_userdata(UserData1(1))?;
    let userdata2 = lua.create_userdata(UserData2(Box::new(2)))?;

    assert!(userdata1.is::<UserData1>());
    assert!(!userdata1.is::<UserData2>());
    assert!(userdata2.is::<UserData2>());
    assert!(!userdata2.is::<UserData1>());

    assert_eq!(userdata1.borrow::<UserData1>()?.0, 1);
    assert_eq!(*userdata2.borrow::<UserData2>()?.0, 2);

    Ok(())
}

#[test]
fn test_methods() -> Result<()> {
    struct MyUserData(i64);

    impl UserData for MyUserData {
        fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
            methods.add_method("get_value", |_, data, ()| Ok(data.0));
            methods.add_method_mut("set_value", |_, data, args| {
                data.0 = args;
                Ok(())
            });
        }
    }

    let lua = make_lua();
    let globals = lua.globals();
    let userdata = lua.create_userdata(MyUserData(42))?;
    globals.set("userdata", userdata.clone())?;
    lua.load(
        r#"
        function get_it()
            return userdata:get_value()
        end

        function set_it(i)
            return userdata:set_value(i)
        end
    "#,
    )
    .exec()?;
    let get = globals.get::<_, Function>("get_it")?;
    let set = globals.get::<_, Function>("set_it")?;
    assert_eq!(get.call::<_, i64>(())?, 42);
    userdata.borrow_mut::<MyUserData>()?.0 = 64;
    assert_eq!(get.call::<_, i64>(())?, 64);
    set.call::<_, ()>(100)?;
    assert_eq!(get.call::<_, i64>(())?, 100);

    Ok(())
}

#[test]
fn test_metamethods() -> Result<()> {
    #[derive(Copy, Clone)]
    struct MyUserData(i64);

    impl UserData for MyUserData {
        fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
            methods.add_method("get", |_, data, ()| Ok(data.0));
            methods.add_meta_function(
                MetaMethod::Add,
                |_, (lhs, rhs): (MyUserData, MyUserData)| Ok(MyUserData(lhs.0 + rhs.0)),
            );
            methods.add_meta_function(
                MetaMethod::Sub,
                |_, (lhs, rhs): (MyUserData, MyUserData)| Ok(MyUserData(lhs.0 - rhs.0)),
            );
            methods.add_meta_method(MetaMethod::Index, |_, data, index: String| {
                if index.to_str()? == "inner" {
                    Ok(data.0)
                } else {
                    Err("no such custom index".to_lua_err())
                }
            });
        }
    }

    let lua = make_lua();
    let globals = lua.globals();
    globals.set("userdata1", MyUserData(7))?;
    globals.set("userdata2", MyUserData(3))?;
    assert_eq!(
        lua.load("userdata1 + userdata2").eval::<MyUserData>()?.0,
        10
    );
    assert_eq!(lua.load("userdata1 - userdata2").eval::<MyUserData>()?.0, 4);
    assert_eq!(lua.load("userdata1:get()").eval::<i64>()?, 7);
    assert_eq!(lua.load("userdata2.inner").eval::<i64>()?, 3);
    assert!(lua.load("userdata2.nonexist_field").eval::<()>().is_err());

    Ok(())
}

#[test]
fn test_gc_userdata() -> Result<()> {
    struct MyUserdata {
        id: u8,
    }

    impl UserData for MyUserdata {
        fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
            methods.add_method("access", |_, this, ()| {
                assert!(this.id == 123);
                Ok(())
            });
        }
    }

    let lua = make_lua();
    lua.globals().set("userdata", MyUserdata { id: 123 })?;

    assert!(lua
        .load(
            r#"
        local tbl = setmetatable({
            userdata = userdata
        }, { __gc = function(self)
            -- resurrect userdata
            hatch = self.userdata
        end })

        tbl = nil
        userdata = nil  -- make table and userdata collectable
        collectgarbage("collect")
        hatch:access()
    "#
        )
        .exec()
        .is_err());

    Ok(())
}

#[test]
fn detroys_userdata() -> Result<()> {
    struct MyUserdata(Arc<()>);

    impl UserData for MyUserdata {}

    let rc = Arc::new(());

    let lua = make_lua();
    lua.globals().set("userdata", MyUserdata(rc.clone()))?;

    assert_eq!(Arc::strong_count(&rc), 2);

    // should destroy all objects
    let _ = lua.globals().raw_remove("userdata")?;
    lua.gc_collect()?;

    assert_eq!(Arc::strong_count(&rc), 1);

    Ok(())
}

#[test]
fn user_value() -> Result<()> {
    struct MyUserData;
    impl UserData for MyUserData {}

    let lua = make_lua();
    let ud = lua.create_userdata(MyUserData)?;
    ud.set_user_value("hello")?;
    assert_eq!(ud.get_user_value::<String>()?, "hello");
    assert!(ud.get_user_value::<u32>().is_err());

    Ok(())
}

#[test]
fn test_functions() -> Result<()> {
    struct MyUserData(i64);

    impl UserData for MyUserData {
        fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
            methods.add_function("get_value", |_, ud: AnyUserData| {
                Ok(ud.borrow::<MyUserData>()?.0)
            });
            methods.add_function("set_value", |_, (ud, value): (AnyUserData, i64)| {
                ud.borrow_mut::<MyUserData>()?.0 = value;
                Ok(())
            });
            methods.add_function("get_constant", |_, ()| Ok(7));
        }
    }

    let lua = make_lua();
    let globals = lua.globals();
    let userdata = lua.create_userdata(MyUserData(42))?;
    globals.set("userdata", userdata.clone())?;
    lua.load(
        r#"
        function get_it()
            return userdata:get_value()
        end

        function set_it(i)
            return userdata:set_value(i)
        end

        function get_constant()
            return userdata.get_constant()
        end
    "#,
    )
    .exec()?;
    let get = globals.get::<_, Function>("get_it")?;
    let set = globals.get::<_, Function>("set_it")?;
    let get_constant = globals.get::<_, Function>("get_constant")?;
    assert_eq!(get.call::<_, i64>(())?, 42);
    userdata.borrow_mut::<MyUserData>()?.0 = 64;
    assert_eq!(get.call::<_, i64>(())?, 64);
    set.call::<_, ()>(100)?;
    assert_eq!(get.call::<_, i64>(())?, 100);
    assert_eq!(get_constant.call::<_, i64>(())?, 7);

    Ok(())
}
