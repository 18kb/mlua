# rlua -- High level bindings between Rust and Lua

[![Build Status](https://travis-ci.org/chucklefish/rlua.svg?branch=master)](https://travis-ci.org/chucklefish/rlua)

[API Documentation](https://docs.rs/rlua)

[Examples](examples/examples.rs)

This library is a high level interface between Rust and Lua.  Its major goal is
to expose as easy to use, practical, and flexible of an API between Rust and Lua
as possible, while also being completely safe.

There are other high level Lua bindings systems for rust, and this crate is an
exploration of a different part of the design space.  The other high level
interface to Lua that I am aware of right now
is [hlua](https://github.com/tomaka/hlua/) which you should definitely check out
and use if it suits your needs.  This crate has the following differences with
hlua:

  * Handles to Lua values use the Lua registry, not the stack
  * Handles to Lua values are all internally mutable
  * Handles to Lua values have non-mutable borrows to the main Lua object, so
    there can be multiple handles or long lived handles
  * Targets Lua 5.3

The key difference here is that rlua handles rust-side references to Lua values
in a fundamentally different way than hlua, more similar to other Lua bindings
systems like [Selene](https://github.com/jeremyong/Selene) for C++.  Values like
LuaTable and LuaFunction that hold onto Lua values in the Rust stack, instead of
pointing at values in the Lua stack, are placed into the registry with luaL_ref.
In this way, it is possible to have an arbitrary number of handles to internal
Lua values at any time, created and destroyed in arbitrary order.  This approach
IS slightly slower than the approach that hlua takes of only manipulating the
Lua stack, but this, combined with internal mutability, allows for a much more
flexible API.

There are currently a few notable missing pieces of this API:

  * Security limits on Lua code such as total instruction limits and recursion
    limits to prevent DOS from malicious Lua code, as well as control over which
    libraries are available to scripts.
  * Lua profiling support
  * "Context" or "Sandboxing" support.  There should be the ability to set the
    `_ENV` upvalue of a loaded chunk to a table other than `_G`, so that you can
    have different environments for different loaded chunks.
  * More fleshed out Lua API, there is some missing nice to have functionality
    not exposed like storing values in the registry, and manipulating `LuaTable`
    metatables.
  * Benchmarks, and quantifying performance differences with what you would
    might write in C.

Additionally, there are ways I would like to change this API, once support lands
in rustc.  For example:

  * Currently, variadics are handled entirely with tuples and traits implemented
    by macro for tuples up to size 12, it would be great if this was replaced
    with real variadic generics when this is available in rust.

It is also worth it to list some non-goals for the project:

  * Be a perfect zero cost wrapper over the Lua C API
  * Allow the user to do absolutely everything that the Lua C API might allow

## API stability or lack thereof

This library is very much Work In Progress, so there is a lot of API churn.  I
believe the library should be stable and usable enough to realistically use in a
real project, but the API has probably not settled down yet.  I currently follow
"pre-1.0 semver" (if such a thing exists), but there have been a large number of
API version bumps, and there will may continue to be.  If you have a dependency
on rlua, you might want to consider adding a 0.x version bound.

## Safety and panics

The goal of this library is complete safety, it should not be possible to cause
undefined behavior whatsoever with the API, even in edge cases.  There is,
however, QUITE a lot of unsafe code in this crate, and I would call the current
safety level of the crate "Work In Progress".  If you find the ability to cause
UB with this API *at all*, please file a bug report.

There are, however, a few ways to cause *panics* and even *aborts* with this
API.  Usually these panics or aborts are alternatives to what would otherwise be
unsafety.

Panic / abort considerations when using this API:

  * The API should be panic safe currently, whenever a panic is generated the
    Lua stack is cleared and the `Lua` instance should continue to be usable.
  * Panic unwinds in Rust callbacks should currently be handled correctly, the
    unwind is caught and carried across the Lua API boundary, and Lua code
    cannot catch rust panics.  This is done by overriding the normal Lua 'pcall'
    and 'xpcall' with custom versions that cannot catch rust panics being piped
    through the normal Lua error system.
  * There are a few panics marked "internal error" that should be impossible to
    trigger.  If you encounter one of these this is a bug.
  * When the internal version of Lua is built using the `gcc` crate (the
    default), `LUA_USE_APICHECK` is enabled.  Any abort caused by this internal
    Lua API checking should be considered a bug.
  * The library internally calls lua_checkstack to ensure that there is
    sufficient stack space, and if the stack cannot be sufficiently grown this
    is a panic.  There should not be a way to cause this using the API, if you
    encounter this, it is a bug.
  * This API attempts only to handle errors in Lua C API functions that can
    cause an error either directly or by running arbitrary Lua code directly,
    not functions that can cause memory errors (marked as 'm' in the Lua C API
    docs).  This means that we must take care to ensure that gc or memory errors
    cannot occur, because this would unsafely longjmp potentially across rust
    frames.  The allocator provided to lua is libc::malloc with an extra guard
    to ensure that OOM errors are immediate aborts, because otherwise this would
    be unsafe.  Similarly, 'setmetatable' is wrapped so that any `__gc`
    metamethod specified in lua scripts will *abort* if the metamethod causes an
    error rather than longjmp like a normal error would.  Lua objects can also
    be resurrected with user provided `__gc` metamethods
    (See [here](https://www.lua.org/manual/5.3/manual.html#2.5.1) for details),
    and this includes userdata, so it is possible to trigger a panic from lua by
    resurrecting a userdata and re-using it after it has been garbage collected.
    It is an eventual goal of the library to ensure that lua scripts cannot
    cause panics or aborts, but currently this is not true and this is a known
    limitation.  Lua scripts should NOT be able to cause unsafety, though, this
    is always considered a bug.
  * There are currently no recursion limits on callbacks.  This could cause one
    of two problems, either the API will run out of stack space and cause a
    panic in Rust, or more likely it will cause an internal `LUA_USE_APICHECK`
    abort, from exceeding LUAI_MAXCCALLS.
  * There are currently no checks on argument sizes, and I think you may be able
    to cause an abort by providing a large enough `LuaVariadic`.
