//! xmas:sqlite: the interface of bun:sqlite, on FrankenSQLite (the `fsqlite` crate, pure Rust).
//!
//! The engine's synchronous API (`fsqlite::blocking`) is what Bun's is shaped like, so each call
//! is a host call that runs on the realm's thread. Rust keeps the connections in a table by id;
//! JS (js/sqlite.js) keeps Database and Statement, and talks to the table with JSON:
//!
//! - parameters go in as a JSON array (positional) or object (named). A blob is {"$b": base64},
//!   an integer beyond 2^53 is {"$i": "digits"}.
//! - a statement's answer comes back as {columns, rows, changes, lastInsertRowid}, rows as arrays,
//!   with the same two tags for blobs and large integers.
//!
//! A statement is prepared on each call. The engine's Statement borrows its connection, and a
//! table of ids cannot hold a borrow.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use fsqlite::blocking::{
    Connection, OpenFlags, params_from_iter,
    types::{Value as Sql, ValueRef},
};
use quickjs_oxide::engine::api::core::{JsString, RuntimeError, Scope, Value};
use serde_json::{Map, Number, Value as Json};

use crate::driver::Driver;

const PRELUDE: &str = include_str!("../../js/sqlite.js");
/// Integers up to this are exact as a JS number.
const SAFE: i64 = 9_007_199_254_740_991;

type Table = Rc<RefCell<(u32, HashMap<u32, Connection>)>>;

fn text(body: &str) -> Result<Value, RuntimeError> {
    let body =
        JsString::try_from_utf8(body).map_err(|error| RuntimeError::Internal(error.to_string()))?;
    Ok(Value::String(body))
}

fn bind_value(value: &Json) -> Result<Sql, String> {
    Ok(match value {
        Json::Null => Sql::Null,
        Json::Bool(flag) => Sql::Integer(i64::from(*flag)),
        Json::Number(number) => match number.as_i64() {
            Some(whole) => Sql::Integer(whole),
            None => Sql::Real(number.as_f64().unwrap_or(f64::NAN)),
        },
        Json::String(string) => Sql::Text(string.clone()),
        Json::Object(object) => {
            if let Some(Json::String(digits)) = object.get("$i") {
                Sql::Integer(
                    digits
                        .parse::<i64>()
                        .map_err(|_| format!("not a 64-bit integer: {digits}"))?,
                )
            } else if let Some(Json::String(data)) = object.get("$b") {
                Sql::Blob(STANDARD.decode(data).map_err(|error| error.to_string())?)
            } else {
                return Err("a parameter must be a number, string, bigint, boolean, null or bytes".into());
            }
        }
        Json::Array(_) => return Err("a parameter cannot be an array".into()),
    })
}

fn out_value(value: ValueRef<'_>) -> Json {
    match value {
        ValueRef::Null => Json::Null,
        ValueRef::Integer(whole) if whole.abs() <= SAFE => Json::Number(whole.into()),
        ValueRef::Integer(whole) => {
            let mut tag = Map::new();
            tag.insert("$i".into(), Json::String(whole.to_string()));
            Json::Object(tag)
        }
        ValueRef::Real(real) => Number::from_f64(real).map_or(Json::Null, Json::Number),
        ValueRef::Text(bytes) => Json::String(String::from_utf8_lossy(bytes).into_owned()),
        ValueRef::Blob(bytes) => {
            let mut tag = Map::new();
            tag.insert("$b".into(), Json::String(STANDARD.encode(bytes)));
            Json::Object(tag)
        }
    }
}

fn run(conn: &Connection, sql: &str, params: &Json) -> Result<Json, String> {
    let mut stmt = conn.prepare(sql).map_err(|error| error.to_string())?;
    let bound: Vec<Sql> = match params {
        Json::Array(items) => items.iter().map(bind_value).collect::<Result<_, _>>()?,
        Json::Object(named) => {
            let mut out = Vec::new();
            for index in 1..=stmt.parameter_count() {
                let Some(name) = stmt.parameter_name(index) else {
                    out.push(Sql::Null);
                    continue;
                };
                let bare = name.trim_start_matches([':', '$', '@']);
                let found = named
                    .get(name)
                    .or_else(|| named.get(bare))
                    .ok_or_else(|| format!("missing parameter {name}"))?;
                out.push(bind_value(found)?);
            }
            out
        }
        _ => Vec::new(),
    };
    let columns: Vec<String> = stmt.column_names().map(str::to_string).collect();
    let mut rows = Vec::new();
    if columns.is_empty() {
        stmt.execute(params_from_iter(bound))
            .map_err(|error| error.to_string())?;
    } else {
        let mut cursor = stmt
            .query(params_from_iter(bound))
            .map_err(|error| error.to_string())?;
        while let Some(row) = cursor.next().map_err(|error| error.to_string())? {
            let mut cells = Vec::with_capacity(columns.len());
            for index in 0..columns.len() {
                cells.push(out_value(
                    row.get_ref(index).map_err(|error| error.to_string())?,
                ));
            }
            rows.push(Json::Array(cells));
        }
    }
    drop(stmt);
    let mut answer = Map::new();
    answer.insert("columns".into(), columns.into());
    answer.insert("rows".into(), Json::Array(rows));
    answer.insert("changes".into(), Json::Number(conn.changes().into()));
    answer.insert(
        "lastInsertRowid".into(),
        out_value(ValueRef::Integer(conn.last_insert_rowid())),
    );
    Ok(Json::Object(answer))
}

pub fn install(driver: &mut Driver) -> Result<(), String> {
    let global = driver
        .context()
        .global_object()
        .map_err(|error| format!("global: {error}"))?;
    let table: Table = Rc::new(RefCell::new((0, HashMap::new())));

    {
        let table = table.clone();
        driver
            .context()
            .define_function(
                &global,
                "__sqlite_open",
                2,
                move |scope: &mut Scope<'_>, path: String, mode: String| {
                    let flags = match mode.as_str() {
                        "readonly" => OpenFlags::SQLITE_OPEN_READ_ONLY,
                        "readwrite" => OpenFlags::SQLITE_OPEN_READ_WRITE,
                        _ => OpenFlags::default(),
                    };
                    let opened = if path == ":memory:" || path.is_empty() {
                        Connection::open_in_memory()
                    } else {
                        Connection::open_with_flags(&path, flags)
                    };
                    match opened {
                        Ok(conn) => {
                            let mut table = table.borrow_mut();
                            table.0 += 1;
                            let id = table.0;
                            table.1.insert(id, conn);
                            Ok(Value::Float(f64::from(id)))
                        }
                        Err(error) => Err(super::throw(scope, &error.to_string())),
                    }
                },
            )
            .map_err(|error| format!("__sqlite_open: {error}"))?;
    }

    {
        let table = table.clone();
        driver
            .context()
            .define_function(
                &global,
                "__sqlite_close",
                1,
                move |_: &mut Scope<'_>, id: f64| {
                    table.borrow_mut().1.remove(&(id as u32));
                    Ok::<Value, RuntimeError>(Value::Undefined)
                },
            )
            .map_err(|error| format!("__sqlite_close: {error}"))?;
    }

    {
        let table = table.clone();
        driver
            .context()
            .define_function(
                &global,
                "__sqlite_run",
                3,
                move |scope: &mut Scope<'_>, id: f64, sql: String, params: String| {
                    let table = table.borrow();
                    let Some(conn) = table.1.get(&(id as u32)) else {
                        return Err(super::throw(scope, "Cannot use a closed database"));
                    };
                    let params: Json = serde_json::from_str(&params).unwrap_or(Json::Null);
                    match run(conn, &sql, &params) {
                        Ok(answer) => text(&answer.to_string()),
                        Err(message) => Err(super::throw(scope, &message)),
                    }
                },
            )
            .map_err(|error| format!("__sqlite_run: {error}"))?;
    }

    {
        let table = table.clone();
        driver
            .context()
            .define_function(
                &global,
                "__sqlite_exec",
                2,
                move |scope: &mut Scope<'_>, id: f64, sql: String| {
                    let table = table.borrow();
                    let Some(conn) = table.1.get(&(id as u32)) else {
                        return Err(super::throw(scope, "Cannot use a closed database"));
                    };
                    match conn.execute_batch(&sql) {
                        Ok(()) => Ok(Value::Undefined),
                        Err(error) => Err(super::throw(scope, &error.to_string())),
                    }
                },
            )
            .map_err(|error| format!("__sqlite_exec: {error}"))?;
    }

    {
        let table = table.clone();
        driver
            .context()
            .define_function(
                &global,
                "__sqlite_columns",
                2,
                move |scope: &mut Scope<'_>, id: f64, sql: String| {
                    let table = table.borrow();
                    let Some(conn) = table.1.get(&(id as u32)) else {
                        return Err(super::throw(scope, "Cannot use a closed database"));
                    };
                    match conn.prepare(&sql) {
                        Ok(stmt) => {
                            let names: Vec<String> =
                                stmt.column_names().map(str::to_string).collect();
                            text(&Json::from(names).to_string())
                        }
                        Err(error) => Err(super::throw(scope, &error.to_string())),
                    }
                },
            )
            .map_err(|error| format!("__sqlite_columns: {error}"))?;
    }

    driver
        .context()
        .eval(PRELUDE)
        .map_err(|error| format!("sqlite prelude: {error}"))?;
    let module = driver
        .context()
        .eval("globalThis.__sqlite_module")
        .map_err(|error| format!("__sqlite_module: {error}"))?;
    let names = ["Database", "Statement", "SQLiteError", "constants", "default"];
    let exports = super::collect(driver, &module, &names)?;
    super::define_name(driver, "xmas:sqlite", exports)
}
