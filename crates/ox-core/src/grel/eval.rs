//! Evaluation: variables, operators (OpenRefine's OperatorCallExpr) and controls.
use super::{
    funcs,
    parse::Expr,
    value::{from_json, Value, Value::*},
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

fn cell_obj(v: Value) -> Value {
    Obj(vec![("value".into(), v), ("recon".into(), Null)])
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
        "cell" => cell_obj(env.value.clone()),
        "rowIndex" => Int(env.row.index() as i64),
        "cells" => cells(env),
        "row" => Obj(vec![
            ("index".into(), Int(env.row.index() as i64)),
            ("cells".into(), cells(env)),
            ("starred".into(), Bool(env.row.starred())),
            ("flagged".into(), Bool(env.row.flagged())),
        ]),
        _ => Null,
    }
}

fn cells(env: &Env) -> Value {
    Obj(env.row.columns().into_iter().map(|c| {
        let v = env.row.cell(&c).unwrap_or(Null);
        (c, cell_obj(v))
    }).collect())
}

pub fn field(o: Value, name: &str) -> Value {
    match o {
        Error(_) => o,
        Obj(fields) => fields.into_iter().find(|(k, _)| k == name).map_or(Null, |(_, v)| v),
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
        Expr::Field(o, name) => field(eval(o, env), name),
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
        (Array(_), _) | (Json(_), _) | (Obj(_), _) => false,
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
                    return Err(Value::err("forRange step must not be zero"));
                }
                let integral = nums.iter().all(|v| matches!(v, Int(_)));
                let n = need_var(3)?;
                let mut out = vec![];
                let mut x = from;
                while (step > 0.0 && x < to) || (step < 0.0 && x > to) {
                    out.push(with_var(env, &n, if integral { Int(x as i64) } else { Num(x) }, &a[4]));
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
