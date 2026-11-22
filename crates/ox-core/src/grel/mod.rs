//! GREL, OpenRefine's expression language: parser, evaluator and function library.
//! Behaviour is pinned to OpenRefine 3.10.1 by tests/grel_conformance.rs.
mod clojure;
pub mod dates;
pub mod eval;
pub mod funcs;
mod language;
pub mod parse;
pub(crate) mod python;
pub mod text;
pub mod value;

pub use eval::{NoRow, Row};
pub use python::release_thread;
pub use value::Value;

/// A compiled expression.
pub struct Program {
    lang: Lang,
    uses_row: bool,
    uses_cell: bool,
}

enum Lang {
    Grel(parse::Expr),
    /// Jython in OpenRefine; the source after its prefix.
    Python(String),
    Clojure(String),
}

impl Program {
    /// Whether the result can depend on more than `value` (other cells, row index...).
    pub fn uses_row(&self) -> bool {
        self.uses_row
    }
    /// Whether it must run row by row on `col`: it reads more than `value`, or reads `cell` on a
    /// reconciled column (equal values can carry different Recons).
    pub fn uses_row_of(&self, col: &crate::table::Column) -> bool {
        self.uses_row || (self.uses_cell && col.recon.is_some())
    }
    pub fn eval(&self, value: Value, row: &dyn Row) -> Value {
        match &self.lang {
            Lang::Grel(expr) => eval::eval(expr, &mut eval::Env::new(value, row)),
            Lang::Python(src) => python::eval(src, value, row),
            Lang::Clojure(src) => clojure::eval(src, value, row),
        }
    }
}

/// Compile an expression as OpenRefine stores it: `grel:` (optional), `jython:`/`python:` or `clojure:`.
pub fn compile(src: &str) -> Result<Program, String> {
    let trimmed = src.trim_start();
    if let Some(body) = trimmed.strip_prefix("jython:").or_else(|| trimmed.strip_prefix("python:")) {
        python::check(body)?;
        return Ok(Program { uses_row: python::reads_row(body), uses_cell: body.contains("cell"), lang: Lang::Python(body.to_string()) });
    }
    if let Some(body) = trimmed.strip_prefix("clojure:") {
        clojure::check(body)?;
        return Ok(Program { uses_row: clojure::reads_row(body), uses_cell: body.contains("cell"), lang: Lang::Clojure(body.to_string()) });
    }
    let expr = parse::parse(src.trim())?;
    Ok(Program { uses_row: eval::uses_row(&expr), uses_cell: eval::uses_cell(&expr), lang: Lang::Grel(expr) })
}
