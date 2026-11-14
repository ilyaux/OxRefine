//! Nonsecret connection profiles, atomically persisted separately from project data.
use crate::{database::{self, Profile}, err, store::Store, Result};
use serde_json::{json, Value as J};
use std::{path::PathBuf, sync::Mutex};

static MUTATION: Mutex<()> = Mutex::new(());
const CONFLICT: &str = "DATABASE_PROFILE_CONFLICT: reload the saved profiles before changing this connection";

fn path(store: &Store) -> PathBuf { store.root().parent().unwrap_or(store.root()).join("database-profiles.json") }
fn read(store: &Store) -> Result<Vec<Profile>> {
    let bytes = match std::fs::read(path(store)) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(_) => return Err("Saved database profiles could not be read".into()),
    };
    let profiles: Vec<J> = serde_json::from_slice(&bytes).map_err(|_| "Saved database profiles are invalid; the file was not changed")?;
    profiles.into_iter().map(database::validate_profile).collect()
}
fn write(store: &Store, profiles: &[Profile]) -> Result<()> {
    crate::store::write_atomic(&path(store), |writer| serde_json::to_writer_pretty(writer, profiles).map_err(std::io::Error::other))
        .map_err(|_| "Saved database profiles could not be written; retry after checking the data directory".into())
}
pub fn list(store: &Store) -> Result<J> { Ok(json!({ "profiles": read(store)?, "drivers": database::drivers() })) }
pub fn save(store: &Store, value: J) -> Result<J> {
    let mut profile = database::validate_profile(value)?;
    let _guard = MUTATION.lock().map_err(err)?;
    let mut profiles = read(store)?;
    if profile.id.is_empty() {
        if !profile.revision.is_empty() { return Err(CONFLICT.into()); }
        if profiles.len() >= 100 { return Err("At most 100 database profiles can be saved".into()); }
        profile.id = format!("{:016x}{:016x}", crate::recon::new_id() as u64, crate::recon::new_id() as u64);
        profile.revision = format!("{:016x}", crate::recon::new_id() as u64);
        profiles.push(profile.clone());
    } else {
        let current = profiles.iter_mut().find(|p| p.id == profile.id).ok_or(CONFLICT)?;
        if profile.revision.is_empty() || current.revision != profile.revision { return Err(CONFLICT.into()); }
        profile.revision = format!("{:016x}", crate::recon::new_id() as u64);
        *current = profile.clone();
    }
    write(store, &profiles)?;
    Ok(json!({ "profile": profile }))
}
pub fn delete(store: &Store, id: &str, revision: &str) -> Result<J> {
    let _guard = MUTATION.lock().map_err(err)?;
    let mut profiles = read(store)?;
    let index = profiles.iter().position(|p| p.id == id).ok_or(CONFLICT)?;
    if revision.is_empty() || profiles[index].revision != revision { return Err(CONFLICT.into()); }
    profiles.remove(index);
    write(store, &profiles)?;
    Ok(json!({ "deleted": id }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn saved_profiles_exclude_secrets_and_compare_revisions_atomically() {
        let work = tempfile::tempdir().unwrap();
        let store = Store::open(work.path()).unwrap();
        let draft = json!({ "name": "Local fixture", "driver": "sqlite", "sqlitePath": work.path().join("fixture.db") });
        let created = save(&store, draft.clone()).unwrap()["profile"].clone();
        let mut secret = draft; secret["password"] = json!("never-save-this-password");
        assert!(save(&store, secret).is_err());
        assert!(!std::fs::read_to_string(path(&store)).unwrap().contains("never-save-this-password"));
        let workers: Vec<_> = (0..2).map(|n| {
            let store = store.clone(); let mut changed = created.clone(); changed["name"] = json!(format!("Edit {n}"));
            std::thread::spawn(move || save(&store, changed))
        }).collect();
        let results: Vec<_> = workers.into_iter().map(|h| h.join().unwrap()).collect();
        assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
        assert!(results.iter().find_map(|r| r.as_ref().err()).unwrap().starts_with("DATABASE_PROFILE_CONFLICT:"));
        let reopened = Store::open(work.path()).unwrap();
        let profiles = list(&reopened).unwrap(); let current = &profiles["profiles"][0];
        assert_ne!(current["revision"], created["revision"]);
        assert!(delete(&reopened, created["id"].as_str().unwrap(), created["revision"].as_str().unwrap()).is_err());
        delete(&reopened, current["id"].as_str().unwrap(), current["revision"].as_str().unwrap()).unwrap();
        assert!(list(&reopened).unwrap()["profiles"].as_array().unwrap().is_empty());
    }
}
