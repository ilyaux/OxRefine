//! Runtime policy for experimental row-code spilling. Dictionaries and operation scratch are separate.
use crate::{spill::CodeBudget, Result};
use std::{cell::RefCell, path::Path, sync::OnceLock};

thread_local! { static ACTIVE: RefCell<Option<CodeBudget>> = const { RefCell::new(None) }; }

pub fn current_budget() -> CodeBudget {
    ACTIVE.with(|active| active.borrow().clone()).unwrap_or_else(|| {
        static DEFAULT: OnceLock<CodeBudget> = OnceLock::new();
        DEFAULT.get_or_init(|| CodeBudget::new(usize::MAX, std::env::temp_dir().join(format!("oxrefine-spill-{}", std::process::id())))).clone()
    })
}

pub fn with_budget<T>(budget: CodeBudget, work: impl FnOnce() -> T) -> T {
    struct Restore(Option<CodeBudget>);
    impl Drop for Restore { fn drop(&mut self) { ACTIVE.with(|active| *active.borrow_mut() = self.0.take()); } }
    let _restore = Restore(ACTIVE.with(|active| active.replace(Some(budget))));
    work()
}

pub fn from_env(default_dir: &Path) -> Result<CodeBudget> {
    let limit = match std::env::var("OXREFINE_CODE_BUDGET_MIB") {
        Ok(value) => value.parse::<usize>().ok().and_then(|mib| mib.checked_mul(1024 * 1024))
            .ok_or("OXREFINE_CODE_BUDGET_MIB must be a nonnegative whole number of MiB")?,
        Err(std::env::VarError::NotPresent) => usize::MAX,
        Err(_) => return Err("OXREFINE_CODE_BUDGET_MIB must be valid text".into()),
    };
    let dir = std::env::var_os("OXREFINE_SPILL_DIR").map(Into::into).unwrap_or_else(|| default_dir.to_path_buf());
    if limit != usize::MAX { std::fs::create_dir_all(&dir).map_err(|e| format!("Cannot create spill directory {}: {e}", dir.display()))?; }
    Ok(CodeBudget::new(limit, dir))
}

pub fn snapshot() -> serde_json::Value {
    let budget = current_budget();
    serde_json::json!({
        "codeHeapLimitBytes": (budget.heap_limit() != usize::MAX).then_some(budget.heap_limit()),
        "codes": budget.stats(), "scope": "row codes only; dictionaries, flags, row indexes and operation scratch remain resident",
        "mappedPages": "OS-managed; mapped bytes are not a process RSS limit"
    })
}
