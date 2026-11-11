//! GREL's cross(): rows of another project whose cell in a column has the same text as a key.
//! The host says how to find projects by name for the length of one command (`scope`); tables and
//! their per-column indexes are kept until it ends.
use crate::{
    grel::{eval::row_value, Value},
    table::Table,
};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
};

type Find = dyn Fn(&str) -> Vec<Table>;

struct Lookup {
    find: Box<Find>,
    tables: RefCell<HashMap<String, Rc<Vec<Table>>>>,
    index: RefCell<HashMap<(String, String), Rc<HashMap<String, Vec<u32>>>>>,
    /// projects being found right now: finding one replays its history, which may cross() again
    loading: RefCell<HashSet<String>>,
}

thread_local! {
    static LOOKUP: RefCell<Option<Rc<Lookup>>> = const { RefCell::new(None) };
}

/// Run `body` with `find` answering cross()'s project names (every project of that name).
pub fn scope<R>(find: impl Fn(&str) -> Vec<Table> + 'static, body: impl FnOnce() -> R) -> R {
    let l = Rc::new(Lookup { find: Box::new(find), tables: Default::default(), index: Default::default(), loading: Default::default() });
    let before = LOOKUP.with(|s| s.borrow_mut().replace(l));
    let out = body();
    LOOKUP.with(|s| *s.borrow_mut() = before);
    out
}

/// The rows of `project` whose `column` reads as `key`, as GREL row objects.
pub fn cross(key: &str, project: &str, column: &str) -> Value {
    let Some(l) = LOOKUP.with(|s| s.borrow().clone()) else { return Value::err("cross needs the projects of this session") };
    let cached = l.tables.borrow().get(project).cloned();
    let tables = match cached {
        Some(t) => t,
        None => {
            if !l.loading.borrow_mut().insert(project.to_string()) {
                return Value::err(format!("cross: project {project} looks itself up while it is being opened"));
            }
            let found = Rc::new((l.find)(project));
            l.loading.borrow_mut().remove(project);
            l.tables.borrow_mut().insert(project.to_string(), found.clone());
            found
        }
    };
    let t = match tables.as_slice() {
        [] => return Value::err(format!("Unable to find project with name: {project}")),
        [t] => t,
        many => return Value::err(format!("Multiple ({}) projects found with name: {project}", many.len())),
    };
    let Ok(ci) = t.col(column) else { return Value::err(format!("Unable to find column {column} in project {project}")) };
    let k = (project.to_string(), column.to_string());
    let cached = l.index.borrow().get(&k).cloned();
    let index = cached.unwrap_or_else(|| {
        let mut ix: HashMap<String, Vec<u32>> = HashMap::new();
        let col = &t.columns[ci];
        for r in 0..t.rows {
            let v = col.get(r);
            if !v.is_null() {
                ix.entry(v.to_java_string()).or_default().push(r as u32);
            }
        }
        let ix = Rc::new(ix);
        l.index.borrow_mut().insert(k, ix.clone());
        ix
    });
    Value::Array(index.get(key).map_or(vec![], |rows| rows.iter().map(|&r| row_value(&t.row(r as usize))).collect()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_rows_by_key() {
        let path = std::env::temp_dir().join(format!("oxrefine-cross-{}.csv", std::process::id()));
        std::fs::write(&path, "city,country\nBoston,USA\nRome,Italy\nBoston,dup\n").unwrap();
        let t = Table::read_csv(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        let n = scope(move |name| if name == "places" { vec![t.clone()] } else { vec![] }, || {
            let Value::Array(rows) = cross("Boston", "places", "city") else { panic!() };
            assert!(matches!(cross("x", "nowhere", "city"), Value::Error(_)));
            assert!(matches!(cross("x", "places", "town"), Value::Error(_)));
            rows.len()
        });
        assert_eq!(n, 2);
        assert!(matches!(cross("Boston", "places", "city"), Value::Error(_)), "no projects outside a scope");
    }
}
