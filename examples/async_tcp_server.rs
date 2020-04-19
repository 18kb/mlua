use std::cell::RefCell;
use std::net::Shutdown;
use std::rc::Rc;

use bstr::BString;
use tokio::net::{TcpListener, TcpStream};
use tokio::prelude::*;
use tokio::task;

use mlua::{Function, Lua, Result, UserData, UserDataMethods};

#[derive(Clone)]
struct LuaTcp;

#[derive(Clone)]
struct LuaTcpListener(Rc<RefCell<TcpListener>>);

#[derive(Clone)]
struct LuaTcpStream(Rc<RefCell<TcpStream>>);

impl UserData for LuaTcp {
    fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
        methods.add_async_function("bind", |_, addr: String| async move {
            let listener = TcpListener::bind(addr).await?;
            Ok(LuaTcpListener(Rc::new(RefCell::new(listener))))
        });
    }
}

impl UserData for LuaTcpListener {
    fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
        methods.add_async_method("accept", |_, listener, ()| async move {
            let (stream, _) = listener.0.borrow_mut().accept().await?;
            Ok(LuaTcpStream(Rc::new(RefCell::new(stream))))
        });
    }
}

impl UserData for LuaTcpStream {
    fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
        methods.add_async_method("peer_addr", |_, stream, ()| async move {
            Ok(stream.0.borrow().peer_addr()?.to_string())
        });

        methods.add_async_method("read", |_, stream, size: usize| async move {
            let mut buf = vec![0; size];
            let n = stream.0.borrow_mut().read(&mut buf).await?;
            buf.truncate(n);
            Ok(BString::from(buf))
        });

        methods.add_async_method("write", |_, stream, data: BString| async move {
            let n = stream.0.borrow_mut().write(&data).await?;
            Ok(n)
        });

        methods.add_method("close", |_, stream, ()| {
            stream.0.borrow().shutdown(Shutdown::Both)?;
            Ok(())
        });
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let lua = Lua::new();

    let spawn = lua.create_function(move |_, func: Function| {
        task::spawn_local(async move { func.call_async::<_, ()>(()).await.unwrap() });
        Ok(())
    })?;

    let globals = lua.globals();
    globals.set("tcp", LuaTcp)?;
    globals.set("spawn", spawn)?;

    let server = lua
        .load(
            r#"
            local addr = ...
            local listener = tcp.bind(addr)
            print("listening on "..addr)
            while true do
                local stream = listener:accept()
                local peer_addr = stream:peer_addr()
                print("connected from "..peer_addr)
                spawn(function()
                    while true do
                        local data = stream:read(100)
                        data = data:match("^%s*(.-)%s*$") -- trim
                        print("["..peer_addr.."] "..data)
                        stream:write("got: "..data.."\n")
                        if data == "exit" then
                            stream:close()
                            break
                        end
                    end
                end)
            end
        "#,
        )
        .into_function()?;

    task::LocalSet::new()
        .run_until(server.call_async::<_, ()>("0.0.0.0:1234"))
        .await
}
