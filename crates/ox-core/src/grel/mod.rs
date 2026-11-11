//! GREL, OpenRefine's expression language: parser, evaluator and function library.
//! Behaviour is pinned to OpenRefine 3.10.1 by tests/grel_conformance.rs.
pub mod dates;
pub mod eval;
pub mod funcs;
pub mod parse;
pub mod text;
pub mod value;

pub use eval::{NoRow, Row};
pub use value::Value;

/// A compiled expression.
pub struct Program {
    expr: parse::Expr,
    uses_row: bool,
    uses_cell: bool,
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
        eval::eval(&self.expr, &mut eval::Env::new(value, row))
    }
}

/// Compile an expression as OpenRefine stores it (`grel:` prefix optional).
pub fn compile(src: &str) -> Result<Program, String> {
    let src = src.trim();
    for lang in ["jython:", "python:", "clojure:"] {
        if src.starts_with(lang) {
            return Err(format!("{} expressions are not supported; rewrite it in GREL", lang.trim_end_matches(':')));
        }
    }
    let expr = parse::parse(src)?;
    Ok(Program { uses_row: eval::uses_row(&expr), uses_cell: eval::uses_cell(&expr), expr })
}
