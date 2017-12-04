use std::os::raw::c_int;

use ffi;
use error::*;
use util::*;
use types::LuaRef;
use lua::{FromLuaMulti, MultiValue, ToLuaMulti};

/// Handle to an internal Lua function.
#[derive(Clone, Debug)]
pub struct Function<'lua>(pub(crate) LuaRef<'lua>);

impl<'lua> Function<'lua> {
    /// Calls the function, passing `args` as function arguments.
    ///
    /// The function's return values are converted to the generic type `R`.
    ///
    /// # Examples
    ///
    /// Call Lua's built-in `tostring` function:
    ///
    /// ```
    /// # extern crate rlua;
    /// # use rlua::{Lua, Function, Result};
    /// # fn try_main() -> Result<()> {
    /// let lua = Lua::new();
    /// let globals = lua.globals();
    ///
    /// let tostring: Function = globals.get("tostring")?;
    ///
    /// assert_eq!(tostring.call::<_, String>(123)?, "123");
    ///
    /// # Ok(())
    /// # }
    /// # fn main() {
    /// #     try_main().unwrap();
    /// # }
    /// ```
    ///
    /// Call a function with multiple arguments:
    ///
    /// ```
    /// # extern crate rlua;
    /// # use rlua::{Lua, Function, Result};
    /// # fn try_main() -> Result<()> {
    /// let lua = Lua::new();
    ///
    /// let sum: Function = lua.eval(r#"
    ///     function(a, b)
    ///         return a + b
    ///     end
    /// "#, None)?;
    ///
    /// assert_eq!(sum.call::<_, u32>((3, 4))?, 3 + 4);
    ///
    /// # Ok(())
    /// # }
    /// # fn main() {
    /// #     try_main().unwrap();
    /// # }
    /// ```
    pub fn call<A: ToLuaMulti<'lua>, R: FromLuaMulti<'lua>>(&self, args: A) -> Result<R> {
        let lua = self.0.lua;
        unsafe {
            stack_err_guard(lua.state, 0, || {
                let args = args.to_lua_multi(lua)?;
                let nargs = args.len() as c_int;
                check_stack(lua.state, nargs + 3);

                ffi::lua_pushcfunction(lua.state, error_traceback);
                let stack_start = ffi::lua_gettop(lua.state);
                lua.push_ref(lua.state, &self.0);
                for arg in args {
                    lua.push_value(lua.state, arg);
                }
                let ret = ffi::lua_pcall(lua.state, nargs, ffi::LUA_MULTRET, stack_start);
                if ret != ffi::LUA_OK {
                    return Err(pop_error(lua.state, ret));
                }
                let nresults = ffi::lua_gettop(lua.state) - stack_start;
                let mut results = MultiValue::new();
                check_stack(lua.state, 1);
                for _ in 0..nresults {
                    results.push_front(lua.pop_value(lua.state));
                }
                ffi::lua_pop(lua.state, 1);
                R::from_lua_multi(results, lua)
            })
        }
    }

    /// Returns a function that, when called, calls `self`, passing `args` as the first set of
    /// arguments.
    ///
    /// If any arguments are passed to the returned function, they will be passed after `args`.
    ///
    /// # Examples
    ///
    /// ```
    /// # extern crate rlua;
    /// # use rlua::{Lua, Function, Result};
    /// # fn try_main() -> Result<()> {
    /// let lua = Lua::new();
    ///
    /// let sum: Function = lua.eval(r#"
    ///     function(a, b)
    ///         return a + b
    ///     end
    /// "#, None)?;
    ///
    /// let bound_a = sum.bind(1)?;
    /// assert_eq!(bound_a.call::<_, u32>(2)?, 1 + 2);
    ///
    /// let bound_a_and_b = sum.bind(13)?.bind(57)?;
    /// assert_eq!(bound_a_and_b.call::<_, u32>(())?, 13 + 57);
    ///
    /// # Ok(())
    /// # }
    /// # fn main() {
    /// #     try_main().unwrap();
    /// # }
    /// ```
    pub fn bind<A: ToLuaMulti<'lua>>(&self, args: A) -> Result<Function<'lua>> {
        unsafe extern "C" fn bind_call_impl(state: *mut ffi::lua_State) -> c_int {
            let nargs = ffi::lua_gettop(state);
            let nbinds = ffi::lua_tointeger(state, ffi::lua_upvalueindex(2)) as c_int;
            check_stack(state, nbinds + 2);

            ffi::lua_settop(state, nargs + nbinds + 1);
            ffi::lua_rotate(state, -(nargs + nbinds + 1), nbinds + 1);

            ffi::lua_pushvalue(state, ffi::lua_upvalueindex(1));
            ffi::lua_replace(state, 1);

            for i in 0..nbinds {
                ffi::lua_pushvalue(state, ffi::lua_upvalueindex(i + 3));
                ffi::lua_replace(state, i + 2);
            }

            ffi::lua_call(state, nargs + nbinds, ffi::LUA_MULTRET);
            ffi::lua_gettop(state)
        }

        let lua = self.0.lua;
        unsafe {
            stack_err_guard(lua.state, 0, || {
                let args = args.to_lua_multi(lua)?;
                let nargs = args.len() as c_int;

                check_stack(lua.state, nargs + 3);
                lua.push_ref(lua.state, &self.0);
                ffi::lua_pushinteger(lua.state, nargs as ffi::lua_Integer);
                for arg in args {
                    lua.push_value(lua.state, arg);
                }

                protect_lua_call(lua.state, nargs + 2, 1, |state| {
                    ffi::lua_pushcclosure(state, bind_call_impl, nargs + 2);
                })?;

                Ok(Function(lua.pop_ref(lua.state)))
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Function;
    use string::String;
    use lua::Lua;

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
        let rust_function = lua.create_function(|_, ()| Ok("hello")).unwrap();

        globals.set("rust_function", rust_function).unwrap();
        assert_eq!(lua_function.call::<_, String>(()).unwrap(), "hello");
    }
}
