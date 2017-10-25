use std::fmt;
use std::error;
use std::panic::catch_unwind;

use {Error, ExternalError, Function, Lua, Result, Table, Thread, ThreadStatus, Value, Variadic};

#[test]
fn test_load() {
    let lua = Lua::new();
    let func = lua.load("return 1+2", None).unwrap();
    let result: i32 = func.call(()).unwrap();
    assert_eq!(result, 3);

    assert!(lua.load("§$%§&$%&", None).is_err());
}

#[test]
fn test_exec() {
    let lua = Lua::new();
    let globals = lua.globals();
    lua.exec::<()>(
        r#"
            res = 'foo'..'bar'
        "#,
        None,
    ).unwrap();
    assert_eq!(globals.get::<_, String>("res").unwrap(), "foobar");

    let module: Table = lua.exec(
        r#"
            local module = {}

            function module.func()
                return "hello"
            end

            return module
        "#,
        None,
    ).unwrap();
    assert!(module.contains_key("func").unwrap());
    assert_eq!(
        module
            .get::<_, Function>("func")
            .unwrap()
            .call::<_, String>(())
            .unwrap(),
        "hello"
    );
}

#[test]
fn test_eval() {
    let lua = Lua::new();
    assert_eq!(lua.eval::<i32>("1 + 1", None).unwrap(), 2);
    assert_eq!(lua.eval::<bool>("false == false", None).unwrap(), true);
    assert_eq!(lua.eval::<i32>("return 1 + 2", None).unwrap(), 3);
    match lua.eval::<()>("if true then", None) {
        Err(Error::SyntaxError {
            incomplete_input: true,
            ..
        }) => {}
        r => panic!(
            "expected SyntaxError with incomplete_input=true, got {:?}",
            r
        ),
    }
}

#[test]
fn test_function() {
    let lua = Lua::new();
    let globals = lua.globals();
    lua.exec::<()>(
        r#"
            function concat(arg1, arg2)
                return arg1 .. arg2
            end
        "#,
        None,
    ).unwrap();

    let concat = globals.get::<_, Function>("concat").unwrap();
    assert_eq!(concat.call::<_, String>(("foo", "bar")).unwrap(), "foobar");
}

#[test]
fn test_bind() {
    let lua = Lua::new();
    let globals = lua.globals();
    lua.exec::<()>(
        r#"
            function concat(...)
                local res = ""
                for _, s in pairs({...}) do
                    res = res..s
                end
                return res
            end
        "#,
        None,
    ).unwrap();

    let mut concat = globals.get::<_, Function>("concat").unwrap();
    concat = concat.bind("foo").unwrap();
    concat = concat.bind("bar").unwrap();
    concat = concat.bind(("baz", "baf")).unwrap();
    assert_eq!(
        concat.call::<_, String>(("hi", "wut")).unwrap(),
        "foobarbazbafhiwut"
    );
}

#[test]
fn test_rust_function() {
    let lua = Lua::new();
    let globals = lua.globals();
    lua.exec::<()>(
        r#"
            function lua_function()
                return rust_function()
            end

            -- Test to make sure chunk return is ignored
            return 1
        "#,
        None,
    ).unwrap();

    let lua_function = globals.get::<_, Function>("lua_function").unwrap();
    let rust_function = lua.create_function(|_, ()| Ok("hello"));

    globals.set("rust_function", rust_function).unwrap();
    assert_eq!(lua_function.call::<_, String>(()).unwrap(), "hello");
}

#[test]
fn test_lua_multi() {
    let lua = Lua::new();
    let globals = lua.globals();
    lua.exec::<()>(
        r#"
            function concat(arg1, arg2)
                return arg1 .. arg2
            end

            function mreturn()
                return 1, 2, 3, 4, 5, 6
            end
        "#,
        None,
    ).unwrap();

    let concat = globals.get::<_, Function>("concat").unwrap();
    let mreturn = globals.get::<_, Function>("mreturn").unwrap();

    assert_eq!(concat.call::<_, String>(("foo", "bar")).unwrap(), "foobar");
    let (a, b) = mreturn.call::<_, (u64, u64)>(()).unwrap();
    assert_eq!((a, b), (1, 2));
    let (a, b, v) = mreturn.call::<_, (u64, u64, Variadic<u64>)>(()).unwrap();
    assert_eq!((a, b), (1, 2));
    assert_eq!(v[..], [3, 4, 5, 6]);
}

#[test]
fn test_coercion() {
    let lua = Lua::new();
    let globals = lua.globals();
    lua.exec::<()>(
        r#"
            int = 123
            str = "123"
            num = 123.0
        "#,
        None,
    ).unwrap();

    assert_eq!(globals.get::<_, String>("int").unwrap(), "123");
    assert_eq!(globals.get::<_, i32>("str").unwrap(), 123);
    assert_eq!(globals.get::<_, i32>("num").unwrap(), 123);
}

#[test]
fn test_error() {
    #[derive(Debug)]
    pub struct TestError;

    impl fmt::Display for TestError {
        fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
            write!(fmt, "test error")
        }
    }

    impl error::Error for TestError {
        fn description(&self) -> &str {
            "test error"
        }

        fn cause(&self) -> Option<&error::Error> {
            None
        }
    }

    let lua = Lua::new();
    let globals = lua.globals();
    lua.exec::<()>(
        r#"
            function no_error()
            end

            function lua_error()
                error("this is a lua error")
            end

            function rust_error()
                rust_error_function()
            end

            function return_error()
                local status, res = pcall(rust_error_function)
                assert(not status)
                return res
            end

            function return_string_error()
                return "this should be converted to an error"
            end

            function test_pcall()
                local testvar = 0

                pcall(function(arg)
                    testvar = testvar + arg
                    error("should be ignored")
                end, 3)

                local function handler(err)
                    testvar = testvar + err
                    return "should be ignored"
                end

                local status, res = xpcall(function()
                    error(5)
                end, handler)
                assert(not status)

                if testvar ~= 8 then
                    error("testvar had the wrong value, pcall / xpcall misbehaving "..testvar)
                end
            end

            function understand_recursion()
                understand_recursion()
            end
        "#,
        None,
    ).unwrap();

    let rust_error_function =
        lua.create_function(|_, ()| -> Result<()> { Err(TestError.to_lua_err()) });
    globals
        .set("rust_error_function", rust_error_function)
        .unwrap();

    let no_error = globals.get::<_, Function>("no_error").unwrap();
    let lua_error = globals.get::<_, Function>("lua_error").unwrap();
    let rust_error = globals.get::<_, Function>("rust_error").unwrap();
    let return_error = globals.get::<_, Function>("return_error").unwrap();
    let return_string_error = globals.get::<_, Function>("return_string_error").unwrap();
    let test_pcall = globals.get::<_, Function>("test_pcall").unwrap();
    let understand_recursion = globals.get::<_, Function>("understand_recursion").unwrap();

    assert!(no_error.call::<_, ()>(()).is_ok());
    match lua_error.call::<_, ()>(()) {
        Err(Error::RuntimeError(_)) => {}
        Err(_) => panic!("error is not RuntimeError kind"),
        _ => panic!("error not returned"),
    }
    match rust_error.call::<_, ()>(()) {
        Err(Error::CallbackError { .. }) => {}
        Err(_) => panic!("error is not CallbackError kind"),
        _ => panic!("error not returned"),
    }

    match return_error.call::<_, Value>(()) {
        Ok(Value::Error(_)) => {}
        _ => panic!("Value::Error not returned"),
    }

    assert!(return_string_error.call::<_, Error>(()).is_ok());

    match lua.eval::<()>("if youre happy and you know it syntax error", None) {
        Err(Error::SyntaxError {
            incomplete_input: false,
            ..
        }) => {}
        Err(_) => panic!("error is not LuaSyntaxError::Syntax kind"),
        _ => panic!("error not returned"),
    }
    match lua.eval::<()>("function i_will_finish_what_i()", None) {
        Err(Error::SyntaxError {
            incomplete_input: true,
            ..
        }) => {}
        Err(_) => panic!("error is not LuaSyntaxError::IncompleteStatement kind"),
        _ => panic!("error not returned"),
    }

    test_pcall.call::<_, ()>(()).unwrap();

    assert!(understand_recursion.call::<_, ()>(()).is_err());

    match catch_unwind(|| -> Result<()> {
        let lua = Lua::new();
        let globals = lua.globals();

        lua.exec::<()>(
            r#"
                function rust_panic()
                    pcall(function () rust_panic_function() end)
                end
            "#,
            None,
        )?;
        let rust_panic_function = lua.create_function(|_, ()| -> Result<()> {
            panic!("expected panic, this panic should be caught in rust")
        });
        globals.set("rust_panic_function", rust_panic_function)?;

        let rust_panic = globals.get::<_, Function>("rust_panic")?;

        rust_panic.call::<_, ()>(())
    }) {
        Ok(Ok(_)) => panic!("no panic was detected, pcall caught it!"),
        Ok(Err(e)) => panic!("error during panic test {:?}", e),
        Err(_) => {}
    };

    match catch_unwind(|| -> Result<()> {
        let lua = Lua::new();
        let globals = lua.globals();

        lua.exec::<()>(
            r#"
                function rust_panic()
                    xpcall(function() rust_panic_function() end, function() end)
                end
            "#,
            None,
        )?;
        let rust_panic_function = lua.create_function(|_, ()| -> Result<()> {
            panic!("expected panic, this panic should be caught in rust")
        });
        globals.set("rust_panic_function", rust_panic_function)?;

        let rust_panic = globals.get::<_, Function>("rust_panic")?;

        rust_panic.call::<_, ()>(())
    }) {
        Ok(Ok(_)) => panic!("no panic was detected, xpcall caught it!"),
        Ok(Err(e)) => panic!("error during panic test {:?}", e),
        Err(_) => {}
    };
}

#[test]
fn test_thread() {
    let lua = Lua::new();
    let thread = lua.create_thread(
        lua.eval::<Function>(
            r#"
                function (s)
                    local sum = s
                    for i = 1,4 do
                        sum = sum + coroutine.yield(sum)
                    end
                    return sum
                end
            "#,
            None,
        ).unwrap(),
    );

    assert_eq!(thread.status(), ThreadStatus::Resumable);
    assert_eq!(thread.resume::<_, i64>(0).unwrap(), 0);
    assert_eq!(thread.status(), ThreadStatus::Resumable);
    assert_eq!(thread.resume::<_, i64>(1).unwrap(), 1);
    assert_eq!(thread.status(), ThreadStatus::Resumable);
    assert_eq!(thread.resume::<_, i64>(2).unwrap(), 3);
    assert_eq!(thread.status(), ThreadStatus::Resumable);
    assert_eq!(thread.resume::<_, i64>(3).unwrap(), 6);
    assert_eq!(thread.status(), ThreadStatus::Resumable);
    assert_eq!(thread.resume::<_, i64>(4).unwrap(), 10);
    assert_eq!(thread.status(), ThreadStatus::Unresumable);

    let accumulate = lua.create_thread(
        lua.eval::<Function>(
            r#"
                function (sum)
                    while true do
                        sum = sum + coroutine.yield(sum)
                    end
                end
            "#,
            None,
        ).unwrap(),
    );

    for i in 0..4 {
        accumulate.resume::<_, ()>(i).unwrap();
    }
    assert_eq!(accumulate.resume::<_, i64>(4).unwrap(), 10);
    assert_eq!(accumulate.status(), ThreadStatus::Resumable);
    assert!(accumulate.resume::<_, ()>("error").is_err());
    assert_eq!(accumulate.status(), ThreadStatus::Error);

    let thread = lua.eval::<Thread>(
        r#"
            coroutine.create(function ()
                while true do
                    coroutine.yield(42)
                end
            end)
        "#,
        None,
    ).unwrap();
    assert_eq!(thread.status(), ThreadStatus::Resumable);
    assert_eq!(thread.resume::<_, i64>(()).unwrap(), 42);

    let thread: Thread = lua.eval(
        r#"
            coroutine.create(function(arg)
                assert(arg == 42)
                local yieldarg = coroutine.yield(123)
                assert(yieldarg == 43)
                return 987
            end)
        "#,
        None,
    ).unwrap();

    assert_eq!(thread.resume::<_, u32>(42).unwrap(), 123);
    assert_eq!(thread.resume::<_, u32>(43).unwrap(), 987);

    match thread.resume::<_, u32>(()) {
        Err(Error::CoroutineInactive) => {}
        Err(_) => panic!("resuming dead coroutine error is not CoroutineInactive kind"),
        _ => panic!("resuming dead coroutine did not return error"),
    }
}

#[test]
fn test_result_conversions() {
    let lua = Lua::new();
    let globals = lua.globals();

    let err = lua.create_function(|_, ()| {
        Ok(Err::<String, _>(
            "only through failure can we succeed".to_lua_err(),
        ))
    });
    let ok = lua.create_function(|_, ()| Ok(Ok::<_, Error>("!".to_owned())));

    globals.set("err", err).unwrap();
    globals.set("ok", ok).unwrap();

    lua.exec::<()>(
        r#"
            local r, e = err()
            assert(r == nil)
            assert(tostring(e) == "only through failure can we succeed")

            local r, e = ok()
            assert(r == "!")
            assert(e == nil)
        "#,
        None,
    ).unwrap();
}

#[test]
fn test_num_conversion() {
    let lua = Lua::new();
    let globals = lua.globals();

    globals.set("n", "1.0").unwrap();
    assert_eq!(globals.get::<_, i64>("n").unwrap(), 1);
    assert_eq!(globals.get::<_, f64>("n").unwrap(), 1.0);
    assert_eq!(globals.get::<_, String>("n").unwrap(), "1.0");

    globals.set("n", "1.5").unwrap();
    assert!(globals.get::<_, i64>("n").is_err());
    assert_eq!(globals.get::<_, f64>("n").unwrap(), 1.5);
    assert_eq!(globals.get::<_, String>("n").unwrap(), "1.5");

    globals.set("n", 1.5).unwrap();
    assert!(globals.get::<_, i64>("n").is_err());
    assert_eq!(globals.get::<_, f64>("n").unwrap(), 1.5);
    assert_eq!(globals.get::<_, String>("n").unwrap(), "1.5");

    lua.exec::<()>("a = math.huge", None).unwrap();
    assert!(globals.get::<_, i64>("n").is_err());
}

#[test]
fn coroutine_from_closure() {
    let lua = Lua::new();
    let thrd_main = lua.create_function(|_, ()| Ok(()));
    lua.globals().set("main", thrd_main).unwrap();
    let thrd: Thread = lua.eval("coroutine.create(main)", None).unwrap();
    thrd.resume::<_, ()>(()).unwrap();
}

#[test]
#[should_panic]
fn coroutine_panic() {
    let lua = Lua::new();
    let thrd_main = lua.create_function(|lua, ()| {
        // whoops, 'main' has a wrong type
        let _coro: u32 = lua.globals().get("main").unwrap();
        Ok(())
    });
    lua.globals().set("main", thrd_main.clone()).unwrap();
    let thrd: Thread = lua.create_thread(thrd_main);
    thrd.resume::<_, ()>(()).unwrap();
}

#[test]
fn test_pcall_xpcall() {
    let lua = Lua::new();
    let globals = lua.globals();

    // make sure that we handle not enough arguments
    assert!(lua.exec::<()>("pcall()", None).is_err());
    assert!(lua.exec::<()>("xpcall()", None).is_err());
    assert!(lua.exec::<()>("xpcall(function() end)", None).is_err());

    // Make sure that the return values from are correct on success
    assert_eq!(lua.eval::<(bool, String)>("pcall(function(p) return p end, 'foo')", None).unwrap(), (true, "foo".to_owned()));
    assert_eq!(lua.eval::<(bool, String)>("xpcall(function(p) return p end, print, 'foo')", None).unwrap(), (true, "foo".to_owned()));

    // Make sure that the return values are correct on errors, and that error handling works

    lua.exec::<()>(
        r#"
            pcall_error = nil
            pcall_status, pcall_error = pcall(error, "testerror")

            xpcall_error = nil
            xpcall_status, _ = xpcall(error, function(err) xpcall_error = err end, "testerror")
        "#,
        None,
    ).unwrap();

    assert_eq!(globals.get::<_, bool>("pcall_status").unwrap(), false);
    assert_eq!(
        globals.get::<_, String>("pcall_error").unwrap(),
        "testerror"
    );

    assert_eq!(globals.get::<_, bool>("xpcall_statusr").unwrap(), false);
    assert_eq!(
        globals.get::<_, String>("xpcall_error").unwrap(),
        "testerror"
    );

    // Make sure that weird xpcall error recursion at least doesn't cause unsafety or panics.
    lua.exec::<()>(
        r#"
            function xpcall_recursion()
                xpcall(error, function(err) error(err) end, "testerror")
            end
        "#,
        None,
    ).unwrap();
    let _ = globals
        .get::<_, Function>("xpcall_recursion")
        .unwrap()
        .call::<_, ()>(());
}

#[test]
#[should_panic]
fn test_recursive_callback_panic() {
    let lua = Lua::new();

    let mut v = Some(Box::new(123));
    let f = lua.create_function::<_, (), _>(move |lua, mutate: bool| {
        if mutate {
            v = None;
        } else {
            // Produce a mutable reference
            let r = v.as_mut().unwrap();
            // Whoops, this will recurse into the function and produce another mutable reference!
            lua.globals()
                .get::<_, Function>("f")
                .unwrap()
                .call::<_, ()>(true)
                .unwrap();
            println!("Should not get here, mutable aliasing has occurred!");
            println!("value at {:p}", r as *mut _);
            println!("value is {}", r);
        }

        Ok(())
    });
    lua.globals().set("f", f).unwrap();
    lua.globals()
        .get::<_, Function>("f")
        .unwrap()
        .call::<_, ()>(false)
        .unwrap();
}

// TODO: Need to use compiletest-rs or similar to make sure these don't compile.
/*
#[test]
fn should_not_compile() {
    let lua = Lua::new();
    let globals = lua.globals();

    // Should not allow userdata borrow to outlive lifetime of AnyUserData handle
    struct MyUserData;
    impl UserData for MyUserData {};
    let userdata_ref;
    {
        let touter = globals.get::<_, Table>("touter").unwrap();
        touter.set("userdata", lua.create_userdata(MyUserData)).unwrap();
        let userdata = touter.get::<_, AnyUserData>("userdata").unwrap();
        userdata_ref = userdata.borrow::<MyUserData>();
    }

    // Should not allow self borrow of lua, it can change addresses
    globals.set("boom", lua.create_function(|_, _| {
        lua.eval::<i32>("1 + 1", None)
    })).unwrap();
}
*/
