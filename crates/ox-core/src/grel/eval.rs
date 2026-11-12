//! Evaluation: variables, operators (OpenRefine's OperatorCallExpr) and controls.
use super::{
    funcs,
    parse::Expr,
    value::{from_json, Class, Value, Value::*},
};
use std::cmp::Ordering;

/// What an expression can see besides `value`: the current row.
pub trait Row {
    fn index(&self) -> usize;
    fn columns(&self) -> Vec<String>;
    fn cell(&self, column: &str) -> Option<Value>;
    fn starred(&self) -> bool {
        false
    }
    fn flagged(&self) -> bool {
        false
    }
    /// facetCount(choice, expression, column); only rows inside a project can answer it.
    fn facet_count(&self, _choice: &Value, _expression: &str, _column: &str) -> Value {
        Value::err("facetCount needs a project")
    }
    /// The Recon of this row's cell in `column`.
    fn recon(&self, _column: &str) -> Option<std::sync::Arc<crate::recon::Recon>> {
        None
    }
    /// The Recon of the cell the expression runs on (`cell.recon`).
    fn current_recon(&self) -> Option<std::sync::Arc<crate::recon::Recon>> {
        None
    }
    /// `row.record`: the record this row belongs to, built with [`record_value`]; null outside a table.
    fn record(&self) -> Value {
        Null
    }
}

/// A record as `row.record` shows it: its number, row range (`to` exclusive), and `cells["Name"]`, the
/// non-blank cells of each column within the record. `cell(row, column)` reads a cell.
pub fn record_value(index: usize, from: usize, to: usize, names: &[String], cell: impl Fn(usize, usize) -> (Value, Option<std::sync::Arc<crate::recon::Recon>>)) -> Value {
    // ponytail: every column is collected on access; fetch one column lazily if huge records get slow
    let cells = names.iter().enumerate().map(|(c, n)| {
        let list = (from..to).map(|r| cell(r, c)).map(|(v, recon)| cell_obj(v, recon)).filter(|c| !c.is_null()).collect();
        (n.clone(), Array(list))
    });
    Obj(Class::Record, vec![
        ("index".into(), Int(index as i64)),
        ("fromRowIndex".into(), Int(from as i64)),
        ("toRowIndex".into(), Int(to as i64)),
        ("rowCount".into(), Int((to - from) as i64)),
        ("cells".into(), Obj(Class::RecordCells, cells.collect())),
    ])
}

/// No row context (constant expressions, per-distinct-value evaluation).
pub struct NoRow;
impl Row for NoRow {
    fn index(&self) -> usize {
        0
    }
    fn columns(&self) -> Vec<String> {
        vec![]
    }
    fn cell(&self, _: &str) -> Option<Value> {
        None
    }
}

pub struct Env<'a> {
    pub value: Value,
    pub row: &'a dyn Row,
    vars: Vec<(String, Value)>,
}

impl<'a> Env<'a> {
    pub fn new(value: Value, row: &'a dyn Row) -> Self {
        Env { value, row, vars: vec![] }
    }
}

/// A cell object; a blank cell is null, as OpenRefine has no cell there.
fn cell_obj(v: Value, recon: Option<std::sync::Arc<crate::recon::Recon>>) -> Value {
    if v.is_null() && recon.is_none() {
        return Null;
    }
    let recon = recon.map_or(Null, |r| crate::recon::View::Recon(r).into());
    Obj(Class::Cell, vec![("value".into(), v), ("recon".into(), recon)])
}

/// A `cell` object an expression returned: its value and Recon (column additions copy both).
pub fn as_cell(v: &Value) -> Option<(Value, Option<std::sync::Arc<crate::recon::Recon>>)> {
    match v {
        Obj(Class::Cell, f) => {
            let recon = match &f[1].1 {
                Value::Recon(r) => match &**r {
                    crate::recon::View::Recon(r) => Some(r.clone()),
                    _ => None,
                },
                _ => None,
            };
            Some((f[0].1.clone(), recon))
        }
        _ => None,
    }
}

fn var(name: &str, env: &Env) -> Value {
    if let Some((_, v)) = env.vars.iter().rev().find(|(n, _)| n == name) {
        return v.clone();
    }
    match name {
        "value" => env.value.clone(),
        "true" => Bool(true),
        "false" => Bool(false),
        "null" => Null,
        "cell" => cell_obj(env.value.clone(), env.row.current_recon()),
        "rowIndex" => Int(env.row.index() as i64),
        "cells" => cells(env.row),
        "row" => row_value(env.row),
        _ => Null,
    }
}

fn cells(row: &dyn Row) -> Value {
    Obj(Class::Cells, row.columns().into_iter().map(|c| {
        let v = row.cell(&c).unwrap_or(Null);
        let recon = row.recon(&c);
        (c, cell_obj(v, recon))
    }).collect())
}

/// A row as GREL's `row` sees it (also what cross() returns for each match).
pub fn row_value(row: &dyn Row) -> Value {
    Obj(Class::Row, vec![
        ("index".into(), Int(row.index() as i64)),
        ("cells".into(), cells(row)),
        ("starred".into(), Bool(row.starred())),
        ("flagged".into(), Bool(row.flagged())),
    ])
}

pub fn field(o: Value, name: &str) -> Value {
    match o {
        Error(_) => o,
        Value::Recon(r) => r.field(name),
        Obj(_, fields) => fields.into_iter().find(|(k, _)| k == name).map_or(Null, |(_, v)| v),
        // a field of an array is that field of each element: row.record.cells["Name"].value
        Array(items) => Array(items.into_iter().map(|v| field(v, name)).collect()),
        Json(j) => match *j {
            serde_json::Value::Object(mut m) => m.remove(name).map_or(Null, from_json),
            _ => Null,
        },
        _ => Null,
    }
}

pub fn eval(e: &Expr, env: &mut Env) -> Value {
    match e {
        Expr::Lit(v) => v.clone(),
        Expr::Regex(r) => Re(Box::new(r.clone())),
        Expr::Var(name) => var(name, env),
        Expr::Array(items) => Array(items.iter().map(|x| eval(x, env)).collect()),
        Expr::Field(o, name) => match (eval(o, env), name.as_str()) {
            // built only when asked for; rows from cross() belong to another project and have none here
            (Obj(Class::Row, f), "record") => match f.first() {
                Some((_, Int(i))) if *i == env.row.index() as i64 => env.row.record(),
                _ => Null,
            },
            (v, name) => field(v, name),
        },
        Expr::Index(o, idx) => {
            let mut args = vec![eval(o, env)];
            args.extend(idx.iter().map(|x| eval(x, env)));
            if let Some(err) = args.iter().find(|v| v.is_error()) {
                return err.clone();
            }
            funcs::call("get", &args)
        }
        Expr::Op(op, a, b) => {
            let a = eval(a, env);
            if a.is_error() {
                return a;
            }
            let b = eval(b, env);
            if b.is_error() {
                return b;
            }
            operator(op, &a, &b)
        }
        Expr::Call(name, args) if funcs::is_control(name) => control(name, args, env),
        // cross(cell or value, project, column): the rows of that project with that text in that column
        Expr::Call(name, args) if name == "cross" => {
            let vals: Vec<Value> = args.iter().map(|x| eval(x, env)).collect();
            if let Some(err) = vals.iter().find(|v| v.is_error()) {
                return err.clone();
            }
            let key = match vals.first() {
                Some(Obj(_, f)) => f.iter().find(|(k, _)| k == "value").map_or(Null, |(_, v)| v.clone()),
                Some(v) => v.clone(),
                None => Null,
            };
            match (&key, vals.get(1), vals.get(2), vals.len()) {
                (Null, ..) | (_, _, _, 4..) => Value::err("cross expects a cell or value, a project name to look up (optional), and a column name in that project (optional)"),
                (k, Some(Str(p)), Some(Str(c)), _) => crate::cross::cross(&k.to_java_string(), p, c),
                _ => Value::err("cross: OxRefine needs both the project name and the column name"),
            }
        }
        Expr::Call(name, args) if name == "facetCount" => {
            let vals: Vec<Value> = args.iter().map(|x| eval(x, env)).collect();
            if let Some(err) = vals.iter().find(|v| v.is_error()) {
                return err.clone();
            }
            match vals.as_slice() {
                [choice, Str(expr), Str(column)] => env.row.facet_count(choice, expr, column),
                _ => Value::err("facetCount expects a choice value, an expression and a column name"),
            }
        }
        Expr::Call(name, args) => {
            let vals: Vec<Value> = args.iter().map(|x| eval(x, env)).collect();
            if let Some(err) = vals.iter().find(|v| v.is_error()) {
                return err.clone();
            }
            funcs::call(name, &vals)
        }
    }
}

fn operator(op: &str, a: &Value, b: &Value) -> Value {
    if op == "+" && !a.is_null() && !b.is_null() && !(a.as_f64().is_some() && b.as_f64().is_some()) {
        return Str(a.to_java_string() + &b.to_java_string());
    }
    match (a, b) {
        (Int(x), Int(y)) => {
            let (x, y) = (*x, *y);
            return match op {
                "+" => Int(x.wrapping_add(y)),
                "-" => Int(x.wrapping_sub(y)),
                "*" => Int(x.wrapping_mul(y)),
                "/" if y == 0 => Num(x as f64 / 0.0),
                "/" => Int(x.wrapping_div(y)),
                "%" if y == 0 => Num(f64::NAN),
                "%" => Int(x.wrapping_rem(y)),
                _ => Bool(cmp_op(op, x.cmp(&y))),
            };
        }
        _ => {}
    }
    if let (Some(x), Some(y)) = (a.as_f64(), b.as_f64()) {
        return match op {
            "+" => Num(x + y),
            "-" => Num(x - y),
            "*" => Num(x * y),
            "/" => Num(x / y),
            "%" => Num(x % y),
            _ => x.partial_cmp(&y).map_or(Bool(op == "!="), |o| Bool(cmp_op(op, o))),
        };
    }
    match op {
        "==" => Bool(same(a, b)),
        "!=" => Bool(!same(a, b)),
        "<" | ">" | "<=" | ">=" => match (a, b) {
            (Str(x), Str(y)) => Bool(cmp_op(op, x.encode_utf16().cmp(y.encode_utf16()))),
            (Date(x), Date(y)) => Bool(cmp_op(op, x.cmp(y))),
            (Bool(x), Bool(y)) => Bool(cmp_op(op, x.cmp(y))),
            _ => Null,
        },
        _ => Null,
    }
}

/// Java equals(): arrays and objects compare by identity, so never equal here.
fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Array(_), _) | (Json(_), _) | (Obj(..), _) => false,
        _ => a == b,
    }
}

fn cmp_op(op: &str, o: Ordering) -> bool {
    match op {
        "==" => o == Ordering::Equal,
        "!=" => o != Ordering::Equal,
        "<" => o == Ordering::Less,
        ">" => o == Ordering::Greater,
        "<=" => o != Ordering::Greater,
        ">=" => o != Ordering::Less,
        _ => false,
    }
}

fn var_name(e: &Expr) -> Option<&str> {
    match e {
        Expr::Var(n) => Some(n),
        _ => None,
    }
}

fn items(v: Value, ctl: &str) -> Result<Vec<Value>, Value> {
    match v {
        Array(a) => Ok(a),
        Json(j) if j.is_array() => match *j {
            serde_json::Value::Array(a) => Ok(a.into_iter().map(from_json).collect()),
            _ => unreachable!(),
        },
        Error(_) => Err(v),
        _ => Err(Value::err(format!("{ctl} expects an array"))),
    }
}

fn with_var(env: &mut Env, name: &str, v: Value, body: &Expr) -> Value {
    env.vars.push((name.to_string(), v));
    let r = eval(body, env);
    env.vars.pop();
    r
}

fn control(name: &str, a: &[Expr], env: &mut Env) -> Value {
    let need_var = |i: usize| var_name(&a[i]).map(str::to_string).ok_or_else(|| Value::err(format!("{name} expects a variable name")));
    let run = |env: &mut Env| -> Result<Value, Value> {
        Ok(match name {
            "if" => match eval(&a[0], env) {
                e @ Error(_) => e,
                Bool(true) => eval(&a[1], env),
                _ => eval(&a[2], env),
            },
            "with" => {
                let v = eval(&a[0], env);
                with_var(env, &need_var(1)?, v, &a[2])
            }
            "forEach" | "filter" => {
                let list = items(eval(&a[0], env), name)?;
                let n = need_var(1)?;
                let mut out = vec![];
                for x in list {
                    let r = with_var(env, &n, x.clone(), &a[2]);
                    if name == "forEach" {
                        out.push(r);
                    } else if matches!(r, Bool(true)) {
                        out.push(x);
                    }
                }
                Array(out)
            }
            "forEachIndex" => {
                let list = items(eval(&a[0], env), name)?;
                let (i, v) = (need_var(1)?, need_var(2)?);
                let mut out = vec![];
                for (k, x) in list.into_iter().enumerate() {
                    env.vars.push((i.clone(), Int(k as i64)));
                    out.push(with_var(env, &v, x, &a[3]));
                    env.vars.pop();
                }
                Array(out)
            }
            "forRange" => {
                let nums: Vec<Value> = a[..3].iter().map(|x| eval(x, env)).collect();
                let [Some(from), Some(to), Some(step)] = [nums[0].as_f64(), nums[1].as_f64(), nums[2].as_f64()] else {
                    return Err(Value::err("forRange expects three numbers"));
                };
                if step == 0.0 {
                    return Err(Value::err("Invalid argument"));
                }
                if ((to - from) / step).ceil() > funcs::MAX_ARRAY as f64 {
                    return Err(Value::err(format!("forRange would run more than {} times", funcs::MAX_ARRAY)));
                }
                let integral = nums.iter().all(|v| matches!(v, Int(_)));
                let n = need_var(3)?;
                let mut out = vec![];
                let mut x = from;
                while (step > 0.0 && x < to) || (step < 0.0 && x > to) {
                    out.push(with_var(env, &n, if integral { Int(x as i64) } else { Num(x) }, &a[4]));
                    if x + step == x {
                        break; // a step too small to move a large number any further
                    }
                    x += step;
                }
                Array(out)
            }
            "forNonBlank" => {
                let v = eval(&a[0], env);
                if v.is_blank() || v.is_error() {
                    eval(&a[3], env)
                } else {
                    with_var(env, &need_var(1)?, v, &a[2])
                }
            }
            "isNull" => Bool(eval(&a[0], env).is_null()),
            "isNotNull" => Bool(!eval(&a[0], env).is_null()),
            "isBlank" => Bool(eval(&a[0], env).is_blank()),
            "isNonBlank" => Bool(!eval(&a[0], env).is_blank()),
            "isEmptyString" => Bool(matches!(eval(&a[0], env), Str(s) if s.is_empty())),
            "isError" => Bool(eval(&a[0], env).is_error()),
            _ => Value::err(format!("unknown control {name}")),
        })
    };
    run(env).unwrap_or_else(|e| e)
}

/// True when the expression reads anything beyond `value` (cells, row, rowIndex...),
/// so it must run once per row instead of once per distinct value.
/// Whether the expression reads `cell` (its Recon differs between rows with equal values).
pub fn uses_cell(e: &Expr) -> bool {
    match e {
        Expr::Var(n) => n == "cell",
        Expr::Lit(_) | Expr::Regex(_) => false,
        Expr::Call(_, a) | Expr::Array(a) => a.iter().any(uses_cell),
        Expr::Field(o, _) => uses_cell(o),
        Expr::Index(o, i) => uses_cell(o) || i.iter().any(uses_cell),
        Expr::Op(_, a, b) => uses_cell(a) || uses_cell(b),
    }
}

pub fn uses_row(e: &Expr) -> bool {
    match e {
        Expr::Var(n) => matches!(n.as_str(), "cells" | "row" | "rowIndex" | "record" | "recon"),
        Expr::Lit(_) | Expr::Regex(_) => false,
        Expr::Call(n, a) => matches!(n.as_str(), "now" | "random" | "randomNumber" | "cross" | "facetCount") || a.iter().any(uses_row),
        Expr::Field(o, _) => uses_row(o),
        Expr::Index(o, i) => uses_row(o) || i.iter().any(uses_row),
        Expr::Op(_, a, b) => uses_row(a) || uses_row(b),
        Expr::Array(a) => a.iter().any(uses_row),
    }
}
