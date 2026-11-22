//! `jython:` / `python:` expressions. OpenRefine runs them in Jython 2.7; OxRefine embeds RustPython
//! (Python 3) with a prelude that restores the Python 2 behaviour common expressions rely on.
//! Behaviour is pinned to OpenRefine 3.10.1 by tests/langs (lang_conformance.rs).
use super::{eval::Row, value::Value};
use chrono::NaiveDate;
use rustpython_vm::{
    builtins::{PyFloat, PyInt, PyList, PyStr, PyTuple},
    compiler::Mode,
    Interpreter, PyObjectRef, VirtualMachine,
};
use std::{cell::RefCell, collections::HashMap};

const PRELUDE: &str = include_str!("python_prelude.py");

struct State {
    interp: Interpreter,
    globals: rustpython_vm::scope::Scope,
    compile: PyObjectRef,
    call: PyObjectRef,
    /// Compiled functions by source; a syntax error is cached as its message.
    funcs: HashMap<String, Result<PyObjectRef, String>>,
    /// Extensions already run in this thread's interpreter, by name, with the version they had.
    loaded: HashMap<String, u64>,
}

thread_local! {
    // One interpreter per thread. ManuallyDrop: RustPython can't be torn down from a thread-local
    // destructor (its own thread-locals are gone by then), so a thread that ends without calling
    // [`release_thread`] leaks it; the server releases it at the end of every request thread.
    // ponytail: compiled functions are cached per thread; bound the cache if needed.
    static STATE: RefCell<Option<std::mem::ManuallyDrop<Box<State>>>> = const { RefCell::new(None) };
}

/// Drop this thread's interpreter, if it has one (call before a short-lived thread ends).
pub fn release_thread() {
    if let Some(state) = STATE.with(|s| s.borrow_mut().take()) {
        drop(std::mem::ManuallyDrop::into_inner(state));
    }
}

fn new_state() -> State {
    let builder = Interpreter::builder(Default::default());
    let defs = rustpython_stdlib::stdlib_module_defs(&builder.ctx);
    let interp = builder.add_native_modules(&defs).add_frozen_modules(rustpython_pylib::FROZEN_STDLIB.decode()).build();
    let (globals, compile, call) = interp.enter(|vm| {
        let scope = vm.new_scope_with_builtins();
        let code = vm.compile(PRELUDE, Mode::Exec, "<prelude>".to_owned()).expect("prelude compiles");
        if let Err(e) = vm.run_code_obj(code, scope.clone()) {
            let mut msg = String::new();
            let _ = vm.write_exception(&mut msg, &e);
            panic!("Python prelude failed: {msg}");
        }
        let compile = scope.globals.get_item("__ox_compile__", vm).unwrap();
        let call = scope.globals.get_item("__ox_call__", vm).unwrap();
        (scope, compile, call)
    });
    State { interp, globals, compile, call, funcs: HashMap::new(), loaded: HashMap::new() }
}

fn with_state<T>(f: impl FnOnce(&mut State) -> T) -> T {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        let state = s.get_or_insert_with(|| std::mem::ManuallyDrop::new(Box::new(new_state())));
        // run extensions installed since this interpreter last looked
        for ext in crate::extensions::sources() {
            if state.loaded.get(&ext.name) != Some(&ext.version) {
                let _ = run_extension(state, &ext.name, &ext.source, &ext.path, ext.version);
            }
        }
        f(state)
    })
}

fn exception(vm: &VirtualMachine, e: &rustpython_vm::builtins::PyBaseExceptionRef) -> String {
    let mut msg = String::new();
    let _ = vm.write_exception(&mut msg, e);
    msg.lines().last().unwrap_or("Python error").to_string()
}

fn helper(state: &State, vm: &VirtualMachine, name: &str) -> PyObjectRef {
    state.globals.globals.get_item(name, vm).expect("prelude helper")
}

fn run_extension(state: &mut State, name: &str, source: &str, path: &str, version: u64) -> Result<String, String> {
    let r = state.interp.enter(|vm| {
        let load = helper(state, vm, "__ox_load_ext__");
        load.call((vm.ctx.new_str(name), vm.ctx.new_str(source), vm.ctx.new_str(path)), vm)
            .map(|j| j.downcast_ref::<PyStr>().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default())
            .map_err(|e| exception(vm, &e))
    });
    state.loaded.insert(name.to_string(), version);
    r
}

/// Run an extension's main.py in this thread and return what it registered (JSON).
pub fn load_extension(name: &str, source: &str, path: &str, version: u64) -> Result<String, String> {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        let state = s.get_or_insert_with(|| std::mem::ManuallyDrop::new(Box::new(new_state())));
        run_extension(state, name, source, path, version)
    })
}

/// A GREL function an extension provides.
pub fn ext_function(name: &str, args: &[Value]) -> Value {
    with_state(|state| {
        state.interp.enter(|vm| {
            let f = helper(state, vm, "__ox_ext_function__");
            let args: PyObjectRef = vm.ctx.new_list(args.iter().map(|a| to_py(vm, a)).collect()).into();
            match f.call((vm.ctx.new_str(name), args), vm) {
                Ok(r) => from_py(vm, &r),
                Err(e) => Value::err(exception(vm, &e)),
            }
        })
    })
}

/// Column names and rows of cells, the shape extensions read and return.
pub type Rows = (Vec<String>, Vec<Vec<Value>>);

fn table_py(vm: &VirtualMachine, columns: &[String], rows: &[Vec<Value>]) -> (PyObjectRef, PyObjectRef) {
    let cols = vm.ctx.new_list(columns.iter().map(|c| vm.ctx.new_str(c.as_str()).into()).collect()).into();
    let rows = vm.ctx.new_list(rows.iter().map(|r| vm.ctx.new_list(r.iter().map(|v| to_py(vm, v)).collect()).into()).collect()).into();
    (cols, rows)
}

fn rows_from_py(vm: &VirtualMachine, r: &PyObjectRef) -> Result<Rows, String> {
    let Value::Array(parts) = from_py(vm, r) else { return Err("the extension returned no table".into()) };
    let [Value::Array(cols), Value::Array(rows)] = &parts[..] else { return Err("the extension returned no table".into()) };
    let columns = cols.iter().map(|c| c.to_java_string()).collect();
    let rows = rows.iter().map(|r| if let Value::Array(cells) = r { cells.clone() } else { vec![] }).collect();
    Ok((columns, rows))
}

pub fn ext_import(id: &str, data: &[u8], options: &serde_json::Value) -> Result<Rows, String> {
    with_state(|state| {
        state.interp.enter(|vm| {
            let f = helper(state, vm, "__ox_ext_import__");
            let r = f.call((vm.ctx.new_str(id), vm.ctx.new_bytes(data.to_vec()), vm.ctx.new_str(options.to_string())), vm).map_err(|e| exception(vm, &e))?;
            rows_from_py(vm, &r)
        })
    })
}

pub fn ext_export(id: &str, columns: &[String], rows: &[Vec<Value>], options: &serde_json::Value) -> Result<Vec<u8>, String> {
    with_state(|state| {
        state.interp.enter(|vm| {
            let f = helper(state, vm, "__ox_ext_export__");
            let (c, r) = table_py(vm, columns, rows);
            let out = f.call((vm.ctx.new_str(id), c, r, vm.ctx.new_str(options.to_string())), vm).map_err(|e| exception(vm, &e))?;
            out.downcast_ref::<rustpython_vm::builtins::PyBytes>().map(|b| b.as_bytes().to_vec()).ok_or_else(|| "the exporter returned no bytes".to_string())
        })
    })
}

pub fn ext_operation(id: &str, columns: &[String], rows: &[Vec<Value>], params: &serde_json::Value) -> Result<Rows, String> {
    with_state(|state| {
        state.interp.enter(|vm| {
            let f = helper(state, vm, "__ox_ext_operation__");
            let (c, r) = table_py(vm, columns, rows);
            let out = f.call((vm.ctx.new_str(id), c, r, vm.ctx.new_str(params.to_string())), vm).map_err(|e| exception(vm, &e))?;
            rows_from_py(vm, &out)
        })
    })
}

fn function(state: &mut State, source: &str) -> Result<PyObjectRef, String> {
    if let Some(f) = state.funcs.get(source) {
        return f.clone();
    }
    let compile = state.compile.clone();
    let f = state.interp.enter(|vm| {
        let name = format!("__temp_{}__", state_hash(source));
        compile.call((vm.ctx.new_str(source), vm.ctx.new_str(name)), vm).map_err(|_| "Syntax error".to_string())
    });
    state.funcs.insert(source.to_string(), f.clone());
    f
}

fn state_hash(s: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

/// Check that `source` compiles (OpenRefine reports "Syntax error" at parse time).
pub fn check(source: &str) -> Result<(), String> {
    with_state(|s| function(s, source).map(|_| ()))
}

/// Whether the source reads other cells or the row, so per-row bindings are worth building.
pub fn reads_row(source: &str) -> bool {
    ["cells", "row"].iter().any(|w| source.contains(w))
}

pub fn eval(source: &str, value: Value, row: &dyn Row) -> Value {
    with_state(|state| {
        let f = match function(state, source) {
            Ok(f) => f,
            Err(e) => return Value::err(e),
        };
        let call = state.call.clone();
        state.interp.enter(|vm| {
            let current = row.current_recon();
            let cell_present = !value.is_null() || current.is_some();
            let current: PyObjectRef = if cell_present { vm.ctx.new_tuple(vec![to_py(vm, &value), recon_py(vm, current)]).into() } else { vm.ctx.none() };
            let (columns, cells) = if reads_row(source) {
                let names = row.columns();
                let cells = names.iter().map(|n| {
                    let v = row.cell(n).unwrap_or(Value::Null);
                    vm.ctx.new_tuple(vec![to_py(vm, &v), recon_py(vm, row.recon(n))]).into()
                });
                let cells: PyObjectRef = vm.ctx.new_list(cells.collect()).into();
                (vm.ctx.new_list(names.iter().map(|n| vm.ctx.new_str(n.as_str()).into()).collect()).into(), cells)
            } else {
                (vm.ctx.none(), vm.ctx.none())
            };
            let args = vec![
                f,
                to_py(vm, &value),
                current,
                columns,
                cells,
                vm.ctx.new_int(row.index()).into(),
                vm.ctx.new_bool(row.starred()).into(),
                vm.ctx.new_bool(row.flagged()).into(),
            ];
            match call.call(args, vm) {
                Ok(r) => from_py(vm, &r),
                Err(e) => {
                    let mut msg = String::new();
                    let _ = vm.write_exception(&mut msg, &e);
                    Value::err(msg.lines().last().unwrap_or("Python error").to_string())
                }
            }
        })
    })
}

fn recon_py(vm: &VirtualMachine, recon: Option<std::sync::Arc<crate::recon::Recon>>) -> PyObjectRef {
    // ponytail: cell.recon is its Java text only; expose judgment/match fields when someone needs them
    match recon {
        Some(r) => vm.ctx.new_str(Value::from(crate::recon::View::Recon(r)).to_java_string()).into(),
        None => vm.ctx.none(),
    }
}

fn to_py(vm: &VirtualMachine, v: &Value) -> PyObjectRef {
    match v {
        Value::Null => vm.ctx.none(),
        Value::Bool(b) => vm.ctx.new_bool(*b).into(),
        Value::Int(i) => vm.ctx.new_int(*i).into(),
        Value::Num(n) => vm.ctx.new_float(*n).into(),
        Value::Str(s) => vm.ctx.new_str(s.as_str()).into(),
        Value::Date(d) => {
            use chrono::{Datelike, Timelike};
            let parts = [d.year() as i64, d.month() as i64, d.day() as i64, d.hour() as i64, d.minute() as i64, d.second() as i64, (d.nanosecond() / 1000) as i64];
            let mut items: Vec<PyObjectRef> = vec![vm.ctx.new_str("date").into()];
            items.extend(parts.iter().map(|p| vm.ctx.new_int(*p).into()));
            vm.ctx.new_tuple(items).into()
        }
        Value::Array(a) => vm.ctx.new_list(a.iter().map(|x| to_py(vm, x)).collect()).into(),
        other => vm.ctx.new_str(other.to_java_string()).into(),
    }
}

fn from_py(vm: &VirtualMachine, o: &PyObjectRef) -> Value {
    if vm.is_none(o) {
        return Value::Null;
    }
    if let Some(s) = o.downcast_ref::<PyStr>() {
        return Value::Str(s.to_string_lossy().into_owned());
    }
    if let Some(i) = o.downcast_ref::<PyInt>() {
        return i64::try_from(i.as_bigint()).map(Value::Int).unwrap_or_else(|_| Value::err("OverflowError: long int too large to convert"));
    }
    if let Some(f) = o.downcast_ref::<PyFloat>() {
        return Value::Num(f.to_f64());
    }
    if let Some(l) = o.downcast_ref::<PyList>() {
        return Value::Array(l.borrow_vec().iter().map(|x| from_py(vm, x)).collect());
    }
    if let Some(t) = o.downcast_ref::<PyTuple>() {
        let items = t.as_slice();
        let tag = items.first().and_then(|t| t.downcast_ref::<PyStr>()).map(|s| s.to_string_lossy().into_owned());
        let text = |i: usize| items.get(i).and_then(|x| x.downcast_ref::<PyStr>()).map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let int = |i: usize| items.get(i).and_then(|x| x.downcast_ref::<PyInt>()).and_then(|x| i64::try_from(x.as_bigint()).ok()).unwrap_or(0);
        return match tag.as_deref() {
            Some("err") => Value::err(text(1)),
            Some("opaque") => Value::Java(text(1)),
            Some("obj") => match text(1).as_str() {
                "Row" => Value::Obj(super::value::Class::Row, vec![]),
                _ => Value::Obj(super::value::Class::Cells, vec![]),
            },
            Some("cell") => Value::Obj(super::value::Class::Cell, vec![("value".into(), items.get(1).map(|x| from_py(vm, x)).unwrap_or(Value::Null)), ("recon".into(), Value::Null)]),
            Some("date") => NaiveDate::from_ymd_opt(int(1) as i32, int(2) as u32, int(3) as u32)
                .and_then(|d| d.and_hms_micro_opt(int(4) as u32, int(5) as u32, int(6) as u32, int(7) as u32))
                .map(Value::Date)
                .unwrap_or_else(|| Value::err("invalid date")),
            _ => Value::Array(items.iter().map(|x| from_py(vm, x)).collect()),
        };
    }
    Value::Java(o.repr(vm).map(|s| s.to_string_lossy().into_owned()).unwrap_or_default())
}
