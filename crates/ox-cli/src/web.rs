//! Explicit browser project selection and optimistic concurrency over one authoritative project cache.
use ox_core::{dispatch, dispatch_free, jobs, store::Store, Project, Session};
use serde_json::{json, Value};
use std::collections::HashMap;

const STARTUP: &str = "startup";
const NONE: &str = "none";
const CACHE_LIMIT: usize = 4;

pub struct Scope {
    pub owner: Option<String>,
    pub project: Option<String>,
    pub revision: Option<String>,
}

impl Scope {
    pub fn new(owner: Option<String>, project: Option<String>, revision: Option<String>) -> Result<Self, String> {
        if owner.as_ref().is_some_and(|id| id.is_empty() || id.len() > 128 || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')) {
            return Err("invalid browser session id".into());
        }
        Ok(Self { owner, project, revision })
    }
}

pub struct Reply(pub u16, pub Value);

pub fn free(cmd: &str, args: &Value, scope: &Scope) -> Option<Reply> {
    if matches!(cmd, "commons_prepare" | "commons_preview" | "database_test" | "database_schema" | "database_prepare" | "database_preview") {
        return jobs::with_owner(scope.owner.as_deref(), || dispatch_free(cmd, args).map(|result| match result {
            Ok(data) => Reply(200, data), Err(error) => Reply(400, json!(error)),
        }));
    }
    let Some(owner) = scope.owner.as_deref() else {
        return dispatch_free(cmd, args).map(|result| match result { Ok(data) => Reply(200, data), Err(error) => Reply(400, json!(error)) });
    };
    let data = match cmd {
        "memory" => json!(ox_core::memory::snapshot()),
        "processes" => jobs::list_owned(owner),
        "cancel_process" => {
            if !jobs::cancel_owned(owner, args["id"].as_u64().unwrap_or(0)) {
                return Some(Reply(403, json!("this job belongs to another browser session")));
            }
            Value::Null
        }
        _ => return None,
    };
    Some(Reply(200, json!({ "data": data, "ms": 0.0 })))
}

struct Cached {
    project: Project,
    used: u64,
    // A failed save can leave useful in-memory changes; unrelated browsing must not discard them.
    pinned: bool,
}

pub struct Projects {
    store: Store,
    cache: HashMap<String, Cached>,
    versions: HashMap<String, u64>,
    epoch: String,
    clock: u64,
    legacy: Option<String>,
    initial: Option<String>,
}

fn global(cmd: &str) -> bool {
    matches!(cmd, "inspect" | "fixed_width_sample" | "json_paths" | "wikitext_tables" | "paste" | "recon_service" | "recon_suggest" | "fetch_url" | "preview_import" | "projects" | "preferences" | "set_preference" | "commons_prepare" | "commons_preview" | "database_profiles" | "database_save_profile" | "database_delete_profile" | "database_test" | "database_schema" | "database_prepare" | "database_preview" | "storage_stats")
}

fn metadata(cmd: &str) -> bool {
    matches!(cmd, "rename_project" | "set_project_info" | "delete_project")
}

fn mutates(cmd: &str) -> bool {
    matches!(cmd, "apply" | "apply_recipe" | "goto") || metadata(cmd)
}

impl Projects {
    pub fn new(mut session: Session, epoch: String) -> Self {
        let store = session.store.take().expect("HTTP sessions have a project store");
        let initial = session.project.as_ref().map(|_| STARTUP.to_string());
        let mut cache = HashMap::new();
        if let Some(project) = session.project.take() { cache.insert(STARTUP.into(), Cached { project, used: 0, pinned: true }); }
        Self { store, cache, versions: HashMap::new(), epoch, clock: 0, legacy: initial.clone(), initial }
    }

    fn revision(&self, id: &str) -> String {
        format!("{}-{}", self.epoch, self.versions.get(id).copied().unwrap_or(0))
    }

    fn context(&self, id: Option<&str>) -> Value {
        json!({ "project": id, "revision": id.map(|id| self.revision(id)) })
    }

    fn load(&mut self, id: &str) -> Result<(), String> {
        if !self.cache.contains_key(id) {
            let (meta, original, history) = self.store.load(id)?;
            let project = Project::restore(meta, original, history)?;
            self.cache.insert(id.into(), Cached { project, used: self.clock, pinned: false });
        }
        Ok(())
    }

    fn prune(&mut self) {
        while self.cache.len() > CACHE_LIMIT {
            let oldest = self.cache.iter().filter(|(id, cached)| id.as_str() != STARTUP && !cached.pinned).min_by_key(|(_, cached)| cached.used).map(|(id, _)| id.clone());
            if let Some(id) = oldest { self.cache.remove(&id); } else { break; }
        }
    }

    pub fn call(&mut self, cmd: &str, args: Value, scope: &Scope) -> Reply {
        // The stdio adapter attaches to this cache, never opens a second Store. Resolve the
        // actual roots so its local file grants cannot be confused with another server's data.
        if cmd == "server_info" {
            let roots = || -> Result<Value, String> {
                let root = self.store.root();
                let data = root.parent().ok_or("missing data directory")?.canonicalize().map_err(|e| e.to_string())?;
                let projects = root.canonicalize().map_err(|e| e.to_string())?;
                let staging = ox_core::import::staging_dir(Some(root));
                let staging = if staging.exists() { staging.canonicalize().map_err(|e| e.to_string())? } else { data.join("staging") };
                Ok(json!({ "dataRoot": data, "projectsRoot": projects, "stagingRoot": staging }))
            };
            return match roots() {
                Ok(data) => Reply(200, json!({ "data": data, "ms": 0.0 })),
                Err(error) => Reply(400, json!(error)),
            };
        }
        self.clock += 1;
        let scoped = scope.owner.is_some();
        let selected = if scoped {
            match scope.project.as_deref() {
                Some(NONE) => None,
                Some(id) => Some(id.to_owned()),
                None if cmd == "summary" => self.initial.clone(),
                None => None,
            }
        } else { self.legacy.clone() };
        let target = if metadata(cmd) || cmd == "open_project" { args["id"].as_str().map(str::to_owned) } else { selected.clone() };
        let needs_project = !global(cmd) && !matches!(cmd, "open" | "import_project" | "commons_create" | "database_create" | "close");

        if needs_project {
            let Some(id) = target.as_deref() else { return Reply(400, json!("no project open")); };
            if let Err(error) = self.load(id) {
                return if scoped { Reply(404, json!({ "code": "project_gone", "message": error, "context": self.context(None) })) }
                    else { Reply(400, json!(error)) };
            }
            if scoped && !matches!(cmd, "summary" | "open_project") && (scope.revision.as_deref() != Some(self.revision(id).as_str()) || metadata(cmd) && scope.project.as_deref() != Some(id)) {
                return Reply(409, json!({ "code": "stale_project", "message": "This project changed in another window. Refresh it before continuing; your command was not applied.", "context": self.context(Some(id)) }));
            }
        }

        // The addressed project is moved, never cloned; other tabs see this same authoritative value.
        let working = if needs_project { target.clone() } else { None };
        let cached = working.as_ref().and_then(|id| self.cache.remove(id));
        let was_pinned = cached.as_ref().is_some_and(|cached| cached.pinned);
        let project = cached.map(|cached| cached.project);
        let mut session = Session { project, store: Some(self.store.clone()) };
        let command = if cmd == "open_project" { "summary" } else { cmd };
        let result = jobs::with_owner(scope.owner.as_deref(), || dispatch(&mut session, command, args));
        let selected_command = matches!(cmd, "open" | "import_project" | "open_project" | "commons_create" | "database_create" | "close");
        let result_id = session.project.as_ref().map(|p| p.meta.as_ref().map(|m| m.id.clone()).unwrap_or_else(|| STARTUP.into()));
        if let Some(project) = session.project.take() {
            let id = result_id.as_ref().unwrap().clone();
            let saved = matches!(cmd, "apply" | "apply_recipe" | "goto" | "set_project_info") && result.is_ok();
            let pinned = was_pinned && !saved || mutates(cmd) && result.is_err();
            self.cache.insert(id, Cached { project, used: self.clock, pinned });
        }
        // A failed recipe may have saved earlier steps, so its attempted mutation invalidates peers too.
        if mutates(cmd) {
            if let Some(id) = &target { *self.versions.entry(id.clone()).or_default() += 1; }
        }
        if cmd == "delete_project" && result.is_ok() {
            if let Some(id) = &target {
                self.cache.remove(id);
                if self.legacy.as_ref() == Some(id) { self.legacy = None; }
            }
        }
        if !scoped && selected_command && result.is_ok() { self.legacy = result_id.clone(); }
        let context_id = if selected_command && result.is_ok() { result_id } else if metadata(cmd) { target } else { selected };
        self.prune();
        match result {
            Ok(mut value) => {
                if cmd == "projects" {
                    let data = &mut value["data"];
                    let projects = if data.is_array() { data.as_array_mut() } else { data["projects"].as_array_mut() };
                    if let Some(projects) = projects {
                        for project in projects {
                            if let Some(id) = project["id"].as_str() { let revision = self.revision(id); project["revision"] = json!(revision); }
                        }
                    }
                }
                value["context"] = self.context(context_id.as_deref());
                Reply(200, value)
            }
            Err(error) if error.starts_with("DATABASE_PROFILE_CONFLICT:") => Reply(409, json!({ "code": "database_profile_conflict", "message": error.trim_start_matches("DATABASE_PROFILE_CONFLICT: ") })),
            Err(error) => Reply(400, json!(error)),
        }
    }
}
