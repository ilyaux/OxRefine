//! Uploading a schema's edits to a Wikibase, as OpenRefine's perform-wikibase-edits does through
//! Wikidata Toolkit: log in (a bot password, or an owner-only OAuth consumer), fetch the existing
//! entities, create new items first, then update existing ones with only what changes, merging
//! statements with those already there. Checked against tests/wikibase/upload.json, OpenRefine
//! 3.10.1 uploading to a mock MediaWiki API (crates/ox-core/tests/wikibase_upload.rs).
use super::{evaluate, language_check, Date, EntityId, Expr, Schema, Statement, TermType, Val};
use crate::Result;
use serde_json::{json, Map, Value as J};
use std::{collections::HashMap, sync::Mutex};

// ------------------------------------------------------------ logging in

#[derive(Clone)]
struct OAuth { consumer_key: String, consumer_secret: String, token: String, token_secret: String }

/// One login to one wiki: its cookies (a bot password) or OAuth credentials, and the user name.
#[derive(Clone)]
struct Session { username: String, cookies: Vec<(String, String)>, oauth: Option<OAuth> }

/// Logins by MediaWiki API, for this process only: credentials are never written anywhere.
static SESSIONS: Mutex<Option<HashMap<String, Session>>> = Mutex::new(None);

fn session(api: &str) -> Option<Session> {
    SESSIONS.lock().unwrap_or_else(|e| e.into_inner()).as_ref()?.get(api).cloned()
}

fn percent(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-._~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

fn hmac_sha1(key: &[u8], message: &[u8]) -> Vec<u8> {
    use sha1::{Digest, Sha1};
    let mut k = if key.len() > 64 { Sha1::digest(key).to_vec() } else { key.to_vec() };
    k.resize(64, 0);
    let pad = |b: u8| k.iter().map(|x| x ^ b).collect::<Vec<u8>>();
    let inner = Sha1::new().chain_update(pad(0x36)).chain_update(message).finalize();
    Sha1::new().chain_update(pad(0x5c)).chain_update(inner).finalize().to_vec()
}

/// OAuth 1.0a's Authorization header (HMAC-SHA1) for a POST of `params` to `url`.
fn oauth_header(url: &str, params: &[(String, String)], o: &OAuth) -> String {
    use base64::Engine;
    let nonce = format!("{:x}{:x}", crate::recon::new_id(), crate::recon::new_id());
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()).to_string();
    let mut oauth = vec![
        ("oauth_consumer_key".to_string(), o.consumer_key.clone()), ("oauth_nonce".into(), nonce), ("oauth_signature_method".into(), "HMAC-SHA1".into()),
        ("oauth_timestamp".into(), stamp), ("oauth_token".into(), o.token.clone()), ("oauth_version".into(), "1.0".into()),
    ];
    let mut all: Vec<(String, String)> = params.iter().cloned().chain(oauth.iter().cloned()).map(|(k, v)| (percent(&k), percent(&v))).collect();
    all.sort();
    let normalized = all.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("&");
    let base = format!("POST&{}&{}", percent(url), percent(&normalized));
    let key = format!("{}&{}", percent(&o.consumer_secret), percent(&o.token_secret));
    let signature = base64::engine::general_purpose::STANDARD.encode(hmac_sha1(key.as_bytes(), base.as_bytes()));
    oauth.push(("oauth_signature".into(), signature));
    format!("OAuth {}", oauth.iter().map(|(k, v)| format!("{k}=\"{}\"", percent(v))).collect::<Vec<_>>().join(", "))
}

/// One MediaWiki API request (a POST form), as the logged-in user when there is a session.
fn call(api: &str, session: Option<&mut Session>, params: &[(&str, &str)]) -> Result<J> {
    send(api, session, params, None)
}

/// A request carrying a file (multipart/form-data): name and contents.
fn send(api: &str, session: Option<&mut Session>, params: &[(&str, &str)], file: Option<(&str, &[u8])>) -> Result<J> {
    let params: Vec<(String, String)> = params.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    let mut request = ureq::post(api).header("User-Agent", concat!("OxRefine/", env!("CARGO_PKG_VERSION"), " (https://github.com/ilyaux/OxRefine)"));
    let cookies = session.as_ref().map(|s| s.cookies.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("; ")).unwrap_or_default();
    if !cookies.is_empty() { request = request.header("Cookie", cookies); }
    // OAuth signs the form fields, none of a multipart body
    if let Some(o) = session.as_ref().and_then(|s| s.oauth.as_ref()) { request = request.header("Authorization", oauth_header(api, if file.is_some() { &[] } else { &params }, o)); }
    let sent = match file {
        None => request.send_form(params.iter().map(|(k, v)| (k.as_str(), v.as_str()))),
        Some((name, bytes)) => {
            let boundary = format!("oxrefine{:x}", crate::recon::new_id());
            let mut body = vec![];
            for (k, v) in &params {
                body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{k}\"\r\nContent-Type: text/plain; charset=UTF-8\r\n\r\n{v}\r\n").as_bytes());
            }
            body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{}\"\r\nContent-Type: application/octet-stream\r\n\r\n", name.replace('"', "")).as_bytes());
            body.extend_from_slice(bytes);
            body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
            request.header("Content-Type", format!("multipart/form-data; boundary={boundary}")).send(&body[..])
        }
    };
    let mut res = sent.map_err(|e| format!("{api}: {e}"))?;
    if let Some(s) = session {
        for header in res.headers().get_all("set-cookie") {
            let Some((name, value)) = header.to_str().ok().and_then(|h| h.split(';').next()).and_then(|kv| kv.split_once('=')) else { continue };
            s.cookies.retain(|(k, _)| k != name.trim());
            s.cookies.push((name.trim().to_string(), value.trim().to_string()));
        }
    }
    let body: J = serde_json::from_reader(res.body_mut().as_reader()).map_err(|e| format!("{api}: {e}"))?;
    if let Some(e) = body.get("error") {
        return Err(format!("[{}] {}", e["code"].as_str().unwrap_or("error"), e["info"].as_str().unwrap_or("")));
    }
    Ok(body)
}

/// Log in to the wiki of `api`: `params` has username and password (a bot password), or
/// consumerToken, consumerSecret, accessToken and accessSecret (an owner-only OAuth consumer).
pub fn login(api: &str, params: &J) -> Result<J> {
    let text = |k: &str| params[k].as_str().unwrap_or("").trim().to_string();
    let mut s = if !text("consumerToken").is_empty() {
        let oauth = OAuth { consumer_key: text("consumerToken"), consumer_secret: text("consumerSecret"), token: text("accessToken"), token_secret: text("accessSecret") };
        let mut s = Session { username: String::new(), cookies: vec![], oauth: Some(oauth) };
        let info = call(api, Some(&mut s), &[("action", "query"), ("meta", "userinfo"), ("format", "json")])?;
        if info["query"]["userinfo"].get("anon").is_some() { return Err("The OAuth credentials were not accepted.".into()); }
        s.username = info["query"]["userinfo"]["name"].as_str().unwrap_or("").to_string();
        s
    } else {
        let mut s = Session { username: text("username"), cookies: vec![], oauth: None };
        let token = call(api, Some(&mut s), &[("action", "query"), ("meta", "tokens"), ("type", "login"), ("format", "json")])?;
        let token = token["query"]["tokens"]["logintoken"].as_str().ok_or("the wiki gave no login token")?.to_string();
        let (name, password) = (text("username"), params["password"].as_str().unwrap_or("").to_string());
        let reply = call(api, Some(&mut s), &[("action", "login"), ("lgname", &name), ("lgpassword", &password), ("lgtoken", &token), ("format", "json")])?;
        if reply["login"]["result"] != "Success" {
            return Err(format!("Login failed: {}", reply["login"]["reason"].as_str().unwrap_or(reply["login"]["result"].as_str().unwrap_or("unknown reason"))));
        }
        s.username = reply["login"]["lgusername"].as_str().unwrap_or(&name).to_string();
        s
    };
    let username = std::mem::take(&mut s.username);
    s.username = username.clone();
    SESSIONS.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(api.to_string(), s);
    Ok(json!({ "logged_in": true, "username": username }))
}

pub fn login_status(api: &str) -> J {
    match session(api) {
        Some(s) => json!({ "logged_in": true, "username": s.username }),
        None => json!({ "logged_in": false }),
    }
}

pub fn logout(api: &str) -> J {
    if let Some(map) = SESSIONS.lock().unwrap_or_else(|e| e.into_inner()).as_mut() { map.remove(api); }
    json!({ "logged_in": false })
}

// ------------------------------------------------------------ entity JSON for wbeditentity

const CALENDAR: &str = "http://www.wikidata.org/entity/";

fn entity_value(id: &str) -> J {
    let kind = match id.chars().next() { Some('P') => "property", Some('M') => "mediainfo", Some('L') => "lexeme", _ => "item" };
    let numeric: u64 = id.trim_start_matches(|c: char| c.is_ascii_alphabetic()).parse().unwrap_or(0);
    json!({ "value": { "id": id, "numeric-id": numeric, "entity-type": kind }, "type": "wikibase-entityid" })
}

fn time(d: &Date) -> String {
    let year = if d.year < 0 { format!("-{:04}", -d.year) } else { format!("+{:04}", d.year) };
    format!("{year}-{:02}-{:02}T00:00:00Z", d.month, d.day)
}

/// A value as Wikibase's JSON datavalue; a new item made earlier in the batch by its new id.
fn datavalue(v: &Val, site: &str, created: &HashMap<i64, String>) -> Option<J> {
    Some(match v {
        Val::Entity(EntityId::Existing(id)) => entity_value(id),
        Val::Entity(EntityId::New(n)) => entity_value(created.get(n)?),
        Val::Str(s) => json!({ "value": s, "type": "string" }),
        Val::Mono { text, lang } => json!({ "value": { "text": text, "language": lang }, "type": "monolingualtext" }),
        Val::Date(d) => json!({ "value": { "time": time(d), "timezone": 0, "before": 0, "after": 0, "precision": d.precision,
            "calendarmodel": format!("{CALENDAR}{}", d.calendar.as_deref().unwrap_or("Q1985727")) }, "type": "time" }),
        Val::Location(lat, lng) => json!({ "value": { "latitude": lat, "longitude": lng, "altitude": null, "precision": 0.0001, "globe": format!("{CALENDAR}Q2") }, "type": "globecoordinate" }),
        Val::Quantity { amount, bounds, unit } => {
            let signed = |d: &super::Decimal| { let p = d.plain(); if p.starts_with('-') { p } else { format!("+{p}") } };
            let mut value = json!({ "amount": signed(amount), "unit": unit.as_ref().map_or_else(|| "1".to_string(), |u| format!("{site}{u}")) });
            if let Some((lower, upper)) = bounds { value["upperBound"] = json!(signed(upper)); value["lowerBound"] = json!(signed(lower)); }
            json!({ "value": value, "type": "quantity" })
        }
    })
}

/// A snak: with its datatype as main snak or qualifier, without it in a reference, as WDTK writes them.
fn snak(pid: &str, datatype: Option<&str>, v: &Val, site: &str, created: &HashMap<i64, String>) -> Option<J> {
    let mut s = Map::new();
    s.insert("property".into(), json!(pid));
    if let Some(t) = datatype { s.insert("datatype".into(), json!(t)); }
    s.insert("datavalue".into(), datavalue(v, site, created)?);
    s.insert("snaktype".into(), json!("value"));
    Some(J::Object(s))
}

fn snak_map(list: &[(String, Val)], datatypes: &HashMap<String, String>, with_type: bool, site: &str, created: &HashMap<i64, String>) -> (Map<String, J>, Vec<String>) {
    let (mut map, mut order) = (Map::new(), vec![]);
    for (pid, v) in list {
        let Some(s) = snak(pid, with_type.then(|| datatypes.get(pid).map(String::as_str).unwrap_or("string")), v, site, created) else { continue };
        if !order.contains(pid) { order.push(pid.clone()); }
        map.entry(pid.clone()).or_insert_with(|| json!([])).as_array_mut().unwrap().push(s);
    }
    (map, order)
}

fn references_json(s: &Statement, datatypes: &HashMap<String, String>, site: &str, created: &HashMap<i64, String>) -> Vec<J> {
    s.references.iter().filter_map(|r| {
        let (snaks, order) = snak_map(r, datatypes, false, site, created);
        (!snaks.is_empty()).then(|| json!({ "snaks": snaks, "snaks-order": order }))
    }).collect()
}

/// A statement in WDTK's key order: rank, id, mainsnak, references, qualifiers-order, type, qualifiers.
fn statement_json(pid: &str, s: &Statement, id: Option<&str>, references: Vec<J>, datatypes: &HashMap<String, String>, site: &str, created: &HashMap<i64, String>) -> Option<J> {
    let mut out = Map::new();
    out.insert("rank".into(), json!("normal"));
    if let Some(id) = id { out.insert("id".into(), json!(id)); }
    out.insert("mainsnak".into(), snak(pid, Some(datatypes.get(pid).map(String::as_str).unwrap_or("string")), s.value.as_ref()?, site, created)?);
    if !references.is_empty() { out.insert("references".into(), J::Array(references)); }
    let (qualifiers, order) = snak_map(&s.qualifiers, datatypes, true, site, created);
    if !qualifiers.is_empty() { out.insert("qualifiers-order".into(), json!(order)); }
    out.insert("type".into(), json!("statement"));
    if !qualifiers.is_empty() { out.insert("qualifiers".into(), J::Object(qualifiers)); }
    Some(J::Object(out))
}

/// A claim's main value and its qualifiers, without ids and hashes, to recognise the same claim.
fn claim_key(claim: &J) -> J {
    let strip = |s: &J| json!({ "property": s["property"], "datavalue": s["datavalue"] });
    let mut qualifiers: Vec<J> = claim["qualifiers"].as_object().into_iter().flatten().flat_map(|(_, list)| list.as_array().into_iter().flatten().map(strip)).collect();
    qualifiers.sort_by_key(|q| q.to_string());
    json!({ "main": strip(&claim["mainsnak"]), "qualifiers": qualifiers })
}

fn reference_key(r: &J) -> J {
    let mut snaks: Vec<J> = r["snaks"].as_object().into_iter().flatten().flat_map(|(_, list)| list.as_array().into_iter().flatten().map(|s| json!({ "property": s["property"], "datavalue": s["datavalue"] }))).collect();
    snaks.sort_by_key(|s| s.to_string());
    J::Array(snaks)
}

fn datatypes(schema: &Schema) -> HashMap<String, String> {
    schema.exprs().into_iter().filter_map(|e| match e {
        Expr::PropConstant { pid, datatype, .. } if !datatype.is_empty() => Some((pid.clone(), datatype.clone())),
        _ => None,
    }).collect()
}

/// Terms by type, the overriding one winning in its language (OpenRefine's merge of a batch).
fn terms(e: &super::Edit, kinds: [TermType; 2]) -> Vec<(String, String, bool)> {
    let mut out: Vec<(String, String, bool)> = vec![];
    for (i, kind) in kinds.iter().enumerate() {
        for (_, lang, text) in e.terms.iter().filter(|(k, _, _)| k == kind) {
            if !out.iter().any(|(l, _, _)| l == lang) { out.push((lang.clone(), text.clone(), i == 1)); }
        }
    }
    out
}

/// The wbeditentity data of one edit: a whole new item, or what changes on an existing one (None
/// when nothing does).
fn edit_data(e: &super::Edit, current: Option<&J>, dt: &HashMap<String, String>, site: &str, created: &HashMap<i64, String>) -> Option<J> {
    let labels = terms(e, [TermType::Label, TermType::LabelIfNew]);
    let descriptions = terms(e, [TermType::Description, TermType::DescriptionIfNew]);
    let aliases: Vec<&(TermType, String, String)> = e.terms.iter().filter(|(k, _, _)| *k == TermType::Alias).collect();
    let Some(current) = current else {
        let term_map = |list: &[(String, String, bool)]| list.iter().map(|(l, t, _)| (l.clone(), json!({ "language": l, "value": t }))).collect::<Map<_, _>>();
        let mut alias_map = Map::new();
        for (_, l, t) in &aliases { alias_map.entry(l.clone()).or_insert_with(|| json!([])).as_array_mut().unwrap().push(json!({ "language": l, "value": t })); }
        let mut claims = Map::new();
        for (pid, statements) in &e.groups {
            for s in statements.iter().filter(|s| s.mode != "delete") {
                if let Some(j) = statement_json(pid, s, None, references_json(s, dt, site, created), dt, site, created) {
                    claims.entry(pid.clone()).or_insert_with(|| json!([])).as_array_mut().unwrap().push(j);
                }
            }
        }
        return Some(json!({ "type": "item", "labels": term_map(&labels), "descriptions": term_map(&descriptions), "aliases": alias_map, "claims": claims, "sitelinks": {} }));
    };
    // an existing entity: only what changes
    let (mut added, mut updated, mut removed) = (vec![], vec![], vec![]);
    let existing = |pid: &str| current["claims"][pid].as_array().or(current["statements"][pid].as_array()).cloned().unwrap_or_default();
    for (pid, statements) in &e.groups {
        for s in statements {
            if s.mode == "delete" {
                for claim in existing(pid) {
                    let matches = match &s.value {
                        None => true,
                        Some(_) => statement_json(pid, s, None, vec![], dt, site, created).is_some_and(|j| claim_key(&j) == claim_key(&claim)),
                    };
                    if matches { if let Some(id) = claim["id"].as_str() { removed.push(json!({ "id": id, "remove": "" })); } }
                }
                continue;
            }
            let references = references_json(s, dt, site, created);
            let Some(new) = statement_json(pid, s, None, references.clone(), dt, site, created) else { continue };
            let same = (s.mode != "add").then(|| existing(pid).into_iter().find(|c| claim_key(c) == claim_key(&new))).flatten();
            match same {
                // the claim is there: add the references it lacks, keep it otherwise
                Some(claim) => {
                    let mut refs = claim["references"].as_array().cloned().unwrap_or_default();
                    let known: Vec<J> = refs.iter().map(reference_key).collect();
                    let fresh: Vec<J> = references.into_iter().filter(|r| !known.contains(&reference_key(r))).collect();
                    if fresh.is_empty() { continue; }
                    refs.extend(fresh);
                    if let Some(j) = statement_json(pid, s, claim["id"].as_str(), refs, dt, site, created) { updated.push(j); }
                }
                None => added.push(new),
            }
        }
    }
    let mut data = Map::new();
    let claims: Vec<J> = added.into_iter().chain(updated).chain(removed).collect();
    if !claims.is_empty() { data.insert("claims".into(), J::Array(claims)); }
    let mut alias_map = Map::new();
    for (_, l, t) in aliases {
        let known = current["aliases"][l].as_array().is_some_and(|a| a.iter().any(|x| x["value"] == **t));
        let listed = alias_map.get(l).and_then(J::as_array).is_some_and(|a| a.iter().any(|x| x["value"] == **t));
        if !known && !listed { alias_map.entry(l.clone()).or_insert_with(|| json!([])).as_array_mut().unwrap().push(json!({ "language": l, "value": t, "add": "" })); }
    }
    if !alias_map.is_empty() { data.insert("aliases".into(), J::Object(alias_map)); }
    for (key, list) in [("descriptions", &descriptions), ("labels", &labels)] {
        // "if new" sets a term only where the entity has none in that language
        let changes: Map<String, J> = list.iter().filter(|(l, t, if_new)| {
            let now = current[key][l]["value"].as_str();
            if *if_new { now.is_none() } else { now != Some(t.as_str()) }
        }).map(|(l, t, _)| (l.clone(), json!({ "language": l, "value": t }))).collect();
        if !changes.is_empty() { data.insert(key.into(), J::Object(changes)); }
    }
    (!data.is_empty()).then_some(J::Object(data))
}

// ------------------------------------------------------------ media files

/// Upload a new file (from a local path, or a web address the wiki fetches) with its page
/// wikitext; its page id and media info id (M + page id).
fn upload_file(api: &str, s: &mut Session, e: &super::Edit, summary: &str, tag: &str, token: &str) -> Result<(u64, String)> {
    let name = e.file_name.as_deref().ok_or("A new file needs a file name")?;
    let path = e.file_path.as_deref().ok_or("A new file needs a file path")?;
    let text = e.wikitext.clone().unwrap_or_default();
    let mut params = vec![("action", "upload"), ("format", "json"), ("assert", "user"), ("filename", name), ("comment", summary), ("text", text.as_str()), ("tags", tag), ("token", token)];
    let reply = if path.starts_with("http://") || path.starts_with("https://") {
        params.push(("url", path));
        call(api, Some(s), &params)?
    } else {
        let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
        send(api, Some(s), &params, Some((name, &bytes)))?
    };
    if reply["upload"]["result"] != "Success" {
        return Err(format!("The wiki did not take the file: {}", reply["upload"]["warnings"]));
    }
    let title = format!("File:{}", reply["upload"]["filename"].as_str().unwrap_or(name));
    let found = call(api, Some(s), &[("action", "query"), ("titles", &title), ("format", "json"), ("assert", "user")])?;
    let pageid = found["query"]["pages"].as_object().into_iter().flatten().find_map(|(_, p)| p["pageid"].as_u64()).ok_or_else(|| format!("{title}: no page after the upload"))?;
    Ok((pageid, format!("M{pageid}")))
}

/// An existing file's page gets the schema's wikitext when the schema says to replace it.
#[allow(clippy::too_many_arguments)]
fn replace_wikitext(api: &str, s: &mut Session, e: &super::Edit, mid: &str, current: &HashMap<String, J>, summary: &str, tag: &str, token: &str, index: &str, cells: &mut [J]) {
    let (Some(text), true) = (&e.wikitext, e.override_wikitext) else { return };
    let Some(pageid) = current.get(mid).and_then(|d| d["pageid"].as_u64()).or_else(|| mid.trim_start_matches('M').parse().ok()) else { return };
    let row = e.rows[0];
    match call(api, Some(s), &[("action", "edit"), ("pageid", &pageid.to_string()), ("text", text), ("summary", summary), ("bot", "true"), ("format", "json"), ("assert", "user"), ("tags", tag), ("token", token)]) {
        Ok(r) => cells[row] = json!(format!("{index}?diff=prev&oldid={}", r["edit"]["newrevid"].as_u64().unwrap_or(0))),
        Err(m) => cells[row] = json!({ "e": m }),
    }
}

// ------------------------------------------------------------ the upload

/// OpenRefine's perform-wikibase-edits settings.
pub struct Settings<'a> { pub summary: &'a str, pub maxlag: u64, pub edit_groups: &'a str, pub tag: &'a str, pub max_edits_per_minute: u64 }

/// What an upload did: the item each new item became (by Recon id) and each row's result
/// (null, the edit's address, or {"e": error}).
pub struct Uploaded { pub created: Vec<(i64, String)>, pub cells: Vec<J> }

/// Upload the edits of `rows` to the schema's Wikibase, as the logged-in user.
pub fn upload(schema: &Schema, t: &crate::table::Table, rows: &[u32], settings: &Settings) -> Result<Uploaded> {
    let api = schema.api.clone().ok_or("The schema has no MediaWiki API")?;
    let mut s = session(&api).ok_or_else(|| format!("Log in to {api} before uploading edits."))?;
    let site = schema.site_iri.clone().unwrap_or_else(|| CALENDAR.into());
    let edits = evaluate(schema, t, rows, &language_check(schema));
    let dt = datatypes(schema);
    // the entities as they are now
    let ids: Vec<String> = edits.iter().filter_map(|e| match &e.subject { EntityId::Existing(id) => Some(id.clone()), _ => None }).collect();
    let mut current: HashMap<String, J> = HashMap::new();
    for chunk in ids.chunks(50) {
        let reply = call(&api, Some(&mut s), &[("action", "wbgetentities"), ("ids", &chunk.join("|")), ("props", "info|datatype|labels|aliases|descriptions|claims|sitelinks"), ("format", "json"), ("assert", "user")])?;
        for (id, doc) in reply["entities"].as_object().into_iter().flatten() { current.insert(id.clone(), doc.clone()); }
    }
    let token = call(&api, Some(&mut s), &[("action", "query"), ("meta", "tokens"), ("type", "csrf"), ("format", "json"), ("assert", "user")])?;
    let token = token["query"]["tokens"]["csrftoken"].as_str().ok_or("the wiki gave no edit token")?.to_string();
    let batch = format!("{:x}", crate::recon::new_id() as u64 & 0xfff_ffff_ffff);
    let summary = match settings.edit_groups.trim() {
        "" => settings.summary.to_string(),
        groups => format!("{} {}", settings.summary, groups.replace("${batch_id}", &batch)).trim().to_string(),
    };
    // the tag template names the OpenRefine version OxRefine follows; tags must exist on the wiki
    let tag = settings.tag.replace("${version}", "3.10");
    let index = api.strip_suffix("api.php").map(|base| format!("{base}index.php")).unwrap_or_else(|| api.clone());
    let mut created: HashMap<i64, String> = HashMap::new();
    let mut cells = vec![J::Null; t.rows];
    let pause = (settings.max_edits_per_minute > 0).then(|| std::time::Duration::from_millis(60_000 / settings.max_edits_per_minute));
    // new items first, so later edits can point at them
    let order: Vec<&super::Edit> = edits.iter().filter(|e| matches!(e.subject, EntityId::New(_))).chain(edits.iter().filter(|e| matches!(e.subject, EntityId::Existing(_)))).collect();
    let mut first = true;
    for (done, e) in order.iter().enumerate() {
        crate::jobs::progress(&format!("Uploading edit {} of {}", done + 1, order.len()), 0);
        crate::jobs::tick(0)?;
        let row = e.rows[0];
        // a new file is uploaded first; its media info then gets the captions and statements
        let mut page = None;
        if let (EntityId::New(n), true) = (&e.subject, e.media_info) {
            match upload_file(&api, &mut s, e, &summary, &tag, &token) {
                Ok((pageid, mid)) => { created.insert(*n, mid.clone()); cells[row] = json!(format!("{site}{mid}")); page = Some(pageid); }
                Err(m) => { cells[row] = json!({ "e": m }); continue; }
            }
        }
        let doc = match (&e.subject, e.media_info) {
            (EntityId::Existing(id), _) => Some(current.get(id).cloned().unwrap_or(J::Null)),
            // the media info of the file just uploaded: there, but empty
            (EntityId::New(_), true) => Some(json!({})),
            (EntityId::New(_), false) => None,
        };
        let target = match &e.subject { EntityId::New(n) if e.media_info => created.get(n).cloned(), EntityId::Existing(id) => Some(id.clone()), _ => None };
        let Some(data) = edit_data(e, doc.as_ref(), &dt, &site, &created) else {
            if let (Some(t), true) = (&target, e.media_info) { replace_wikitext(&api, &mut s, e, t, &current, &summary, &tag, &token, &index, &mut cells); }
            continue;
        };
        if !first { if let Some(p) = pause { std::thread::sleep(p); } }
        first = false;
        let data = data.to_string();
        let maxlag = settings.maxlag.to_string();
        let mut params: Vec<(&str, &str)> = vec![("action", "wbeditentity"), ("format", "json"), ("summary", &summary), ("maxlag", &maxlag), ("bot", ""), ("assert", "user"), ("token", &token), ("data", &data), ("tags", &tag)];
        let base = doc.as_ref().and_then(|d| d["lastrevid"].as_u64()).map(|r| r.to_string());
        match (&target, &base) {
            (None, _) => params.push(("new", "item")),
            (Some(id), Some(rev)) => { params.push(("id", id)); params.push(("baserevid", rev)); }
            (Some(id), None) => params.push(("id", id)),
        }
        // the wiki asks to wait while it is lagging
        let mut reply = call(&api, Some(&mut s), &params);
        for _ in 0..5 {
            if !matches!(&reply, Err(m) if m.starts_with("[maxlag]")) { break; }
            std::thread::sleep(std::time::Duration::from_secs(5));
            reply = call(&api, Some(&mut s), &params);
        }
        match reply {
            Ok(r) if e.media_info => {
                // the new file's page shows its structured data once purged; an existing file may get new wikitext
                if let Some(pageid) = page {
                    let _ = call(&api, Some(&mut s), &[("action", "purge"), ("pageids", &pageid.to_string()), ("format", "json"), ("assert", "user"), ("errorformat", "raw")]);
                } else if let Some(t) = &target {
                    cells[row] = json!(format!("{index}?diff=prev&oldid={}", r["entity"]["lastrevid"].as_u64().unwrap_or(0)));
                    replace_wikitext(&api, &mut s, e, t, &current, &summary, &tag, &token, &index, &mut cells);
                }
            }
            Ok(r) => {
                let id = r["entity"]["id"].as_str().unwrap_or("").to_string();
                if let EntityId::New(n) = e.subject {
                    cells[row] = json!(format!("{site}{id}"));
                    created.insert(n, id);
                } else {
                    cells[row] = json!(format!("{index}?diff=prev&oldid={}", r["entity"]["lastrevid"].as_u64().unwrap_or(0)));
                }
            }
            Err(m) => cells[row] = json!({ "e": m }),
        }
    }
    Ok(Uploaded { created: created.into_iter().collect(), cells })
}
