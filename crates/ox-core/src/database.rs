//! Explicit, bounded database reads. Profiles contain no credentials; prepared imports contain no SQL.
use crate::{grel::Value, jobs, table::{Column, Interner, Table}, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as J};
use std::{collections::VecDeque, path::Path, sync::{Arc, Mutex, OnceLock, atomic::{AtomicBool, Ordering}}, time::{Duration, Instant}};
#[path="database_driver.rs"]
mod driver;

const MAX_BYTES: usize = 128 * 1024 * 1024;
const TTL: Duration = Duration::from_secs(1800);
const PREVIEW_CHARS: usize = 2000;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all="camelCase", default, deny_unknown_fields)]
pub struct Profile {
    pub id: String, pub revision: String, pub name: String, pub driver: String,
    pub host: String, pub port: u16, pub database: String, pub username: String,
    pub sqlite_path: String, pub tls: String,
}
impl Default for Profile {
    fn default()->Self {Self{id:String::new(),revision:String::new(),name:String::new(),driver:"sqlite".into(),host:"localhost".into(),port:0,database:String::new(),username:String::new(),sqlite_path:String::new(),tls:"verify-full".into()}}
}
pub fn validate_profile(value:J)->Result<Profile> {
    let mut profile:Profile=serde_json::from_value(value).map_err(|_|"Invalid database profile fields; passwords belong only in per-request credentials")?;
    profile.name=profile.name.trim().into();
    if profile.name.is_empty()||profile.name.len()>200||profile.name.chars().any(char::is_control){return Err("A database profile needs a name of at most 200 characters".into());}
    if !matches!(profile.driver.as_str(),"sqlite"|"postgres"|"mysql"|"mariadb"){return Err("Supported database drivers are sqlite, postgres, mysql and mariadb".into());}
    if !matches!(profile.tls.as_str(),"verify-full"|"disable"){return Err("TLS must be verify-full or disable".into());}
    for (name,value,limit) in [("id",&profile.id,64),("revision",&profile.revision,64),("host",&profile.host,253),("database",&profile.database,512),("username",&profile.username,512),("sqlitePath",&profile.sqlite_path,4096)] {
        if value.len()>limit||value.chars().any(char::is_control){return Err(format!("Invalid database profile {name}"));}
    }
    if profile.driver=="sqlite" {
        if profile.sqlite_path.is_empty(){return Err("Choose an existing SQLite database file".into());}
        if ["//","\\\\","\\/","/\\"].iter().any(|prefix|profile.sqlite_path.starts_with(*prefix))||profile.sqlite_path.get(..5).is_some_and(|s|s.eq_ignore_ascii_case("file:")){return Err("Choose a local SQLite file; network paths and SQLite connection URIs are not accepted".into());}
        profile.host.clear();profile.port=0;profile.database.clear();profile.username.clear();
    }else{
        if profile.host.is_empty()||profile.host.contains(['/', '\\', '@', '?', '#'])||profile.database.is_empty()||profile.username.is_empty(){return Err("Enter a database host, database name and username; connection URLs are not accepted".into());}
        if profile.port==0 {profile.port=if profile.driver=="postgres"{5432}else{3306};}
        profile.sqlite_path.clear();
    }
    Ok(profile)
}
pub fn drivers()->J {json!([{"id":"sqlite","label":"SQLite","available":true},{"id":"postgres","label":"PostgreSQL","available":true},{"id":"mysql","label":"MySQL","available":true},{"id":"mariadb","label":"MariaDB","available":true}])}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Credentials {#[serde(default)]password:String}
#[derive(Deserialize)]
#[serde(rename_all="camelCase", deny_unknown_fields)]
struct Request {
    profile:J, #[serde(default)]credentials:Credentials, request_id:String,
    #[serde(default)]sql:String, #[serde(default="default_rows")]max_rows:usize,
    #[serde(default="default_timeout")]timeout_seconds:u64,
}
fn default_rows()->usize {100_000}
fn default_timeout()->u64 {30}
const DENIED_FUNCTIONS:&[&str]=&["get_lock","release_lock","release_all_locks","load_file","load_extension","writefile","readfile","lo_import","lo_export","dblink","dblink_exec","pg_terminate_backend","pg_cancel_backend","pg_reload_conf","pg_rotate_logfile","pg_advisory_lock","pg_advisory_xact_lock","pg_try_advisory_lock","pg_try_advisory_xact_lock","pg_logical_emit_message","set_config","nextval","setval"];
impl Request {
    fn parse(params:J)->Result<(Self,Profile)> {
        let request:Self=serde_json::from_value(params).map_err(|_|"Invalid database request fields")?;
        if request.request_id.is_empty()||request.request_id.len()>64||!request.request_id.bytes().all(|b|b.is_ascii_alphanumeric()||matches!(b,b'-'|b'_')){return Err("Database requestId must contain 1–64 letters, digits, hyphens or underscores".into());}
        if !(1..=1_000_000).contains(&request.max_rows){return Err("maxRows must be between 1 and 1000000".into());}
        if !(1..=120).contains(&request.timeout_seconds){return Err("timeoutSeconds must be between 1 and 120".into());}
        if request.credentials.password.len()>16384{return Err("Database password is too long".into());}
        let profile=validate_profile(request.profile.clone())?;Ok((request,profile))
    }
}

/// The server transaction is authoritative. This guard also excludes multi-statements and SQL
/// constructs with effects outside a read-only transaction (files, locks and session settings).
fn read_query(sql:&str,driver:&str)->Result<String> {
    if sql.len()>64*1024{return Err("Database query exceeds 64 KiB".into());}
    let mut words=vec![];let bytes=sql.as_bytes();let mut i=0;let mut end=sql.len();let mut terminated=false;
    while i<bytes.len(){
        if bytes[i].is_ascii_whitespace(){i+=1;continue;}
        if bytes[i..].starts_with(b"--"){if bytes.get(i+2).is_some_and(|b|!b.is_ascii_whitespace()){return Err("SQL line comments require whitespace after --".into());}i+=2;while i<bytes.len()&&bytes[i]!=b'\n'{i+=1;}continue;}
        if bytes[i..].starts_with(b"/*"){
            if matches!(bytes.get(i+2),Some(b'!'|b'+'))||(matches!(bytes.get(i+2),Some(b'M'|b'm'))&&bytes.get(i+3)==Some(&b'!')){return Err("Executable SQL comments and optimizer hints are not supported".into());}
            i+=2;let mut depth=1;while i<bytes.len()&&depth>0{if bytes[i..].starts_with(b"/*"){return Err("Nested SQL comments are not supported".into());}else if bytes[i..].starts_with(b"*/"){depth-=1;i+=2;}else{i+=1;}}if depth>0{return Err("Unclosed SQL comment".into());}continue;
        }
        if terminated{return Err("Only one read-only SQL statement is accepted".into());}
        if bytes[i]==b';'{terminated=true;end=i;i+=1;continue;}
        if matches!(bytes[i],b'\''|b'"'|b'`')||(bytes[i]==b'['&&driver=="sqlite"){
            let quote=bytes[i];let close=if quote==b'['{b']'}else{quote};let start=i+1;i+=1;
            loop {if i==bytes.len(){return Err("Unclosed SQL string or identifier".into());}if bytes[i]==close{if bytes.get(i+1)==Some(&close){i+=2;continue;}break;}if bytes[i]==b'\\'{return Err("Backslash-escaped SQL literals are not supported; use standard doubled quotes".into());}i+=1;}
            if quote!=b'\''&&DENIED_FUNCTIONS.contains(&sql[start..i].to_ascii_lowercase().as_str()){return Err("Database import does not allow that SQL function".into());}i+=1;continue;
        }
        if bytes[i]==b'$'{return Err("Dollar-quoted SQL is not supported; use standard single-quoted strings".into());}
        if bytes[i..].starts_with(b":=")||bytes[i]==b'@'{return Err("SQL session-variable assignments are not supported".into());}
        if bytes[i].is_ascii_alphabetic()||bytes[i]==b'_'{let start=i;i+=1;while i<bytes.len()&&(bytes[i].is_ascii_alphanumeric()||bytes[i]==b'_'){i+=1;}words.push(sql[start..i].to_ascii_lowercase());}else{i+=1;}
    }
    if !matches!(words.first().map(String::as_str),Some("select"|"with")){return Err("Database import accepts a SELECT query, optionally starting with WITH".into());}
    const DENIED:&[&str]=&["insert","update","delete","merge","replace","create","drop","alter","truncate","grant","revoke","commit","rollback","begin","start","set","call","do","copy","into","outfile","dumpfile","load","attach","detach","pragma","vacuum","analyze","lock","unlock","for"];
    if let Some(word)=words.iter().find(|w|DENIED.contains(&w.as_str())||DENIED_FUNCTIONS.contains(&w.as_str())){return Err(format!("Database import does not allow the SQL construct {word}"));}
    Ok(sql[..end].trim().into())
}

#[derive(Clone)]
pub struct Prepared {pub table:Table,pub name:String,pub profile_id:String,pub profile_revision:String,pub driver:String}
#[derive(Clone)]
struct Snapshot {prepared:Prepared,id:String,types:Vec<String>,warnings:Vec<String>,bytes:usize,created:Instant}
#[derive(Default)]
struct Cache {items:VecDeque<Snapshot>}
impl Cache {
    fn prune(&mut self){self.items.retain(|s|s.created.elapsed()<TTL);}
    fn insert(&mut self,mut snapshot:Snapshot)->Result<Snapshot>{self.prune();if snapshot.bytes>MAX_BYTES{return Err("Database result exceeds the 128 MiB snapshot budget".into());}while !self.items.is_empty()&&(self.items.len()>=4||self.items.iter().map(|s|s.bytes).sum::<usize>()+snapshot.bytes>MAX_BYTES){self.items.pop_front();}snapshot.id=format!("{:016x}{:016x}",crate::recon::new_id()as u64,crate::recon::new_id()as u64);self.items.push_back(snapshot.clone());Ok(snapshot)}
    fn get(&mut self,id:&str)->Result<Snapshot>{self.prune();self.items.iter().find(|s|s.id==id).cloned().ok_or_else(||"This database preview expired, was evicted or belongs to an earlier server run. Run the query again.".into())}
}
fn cache()->&'static Mutex<Cache>{static CACHE:OnceLock<Mutex<Cache>>=OnceLock::new();CACHE.get_or_init(||Mutex::new(Cache::default()))}
pub fn prepared(id:&str)->Result<Prepared>{Ok(cache().lock().unwrap_or_else(|e|e.into_inner()).get(id)?.prepared)}
pub fn preview(id:&str,start:usize,limit:usize)->Result<J>{view(&cache().lock().unwrap_or_else(|e|e.into_inner()).get(id)?,start,limit)}
fn view(snapshot:&Snapshot,start:usize,limit:usize)->Result<J>{
    if limit>100{return Err("Database preview is limited to 100 rows per page".into());}let limit=if limit==0{20}else{limit};let t=&snapshot.prepared.table;let mut shortened=false;
    let rows=(start.min(t.rows)..t.rows).take(limit).map(|r|{let mut types=String::new();let cells=t.columns.iter().map(|c|match c.get(r){Value::Null=>{types.push('s');J::Null},Value::Int(n)=>{types.push('n');json!(n.to_string())},Value::Num(n)=>{types.push('n');json!(Value::Num(*n).display())},Value::Bool(b)=>{types.push('b');json!(b)},value=>{types.push('s');let s=value.display();if s.chars().count()>PREVIEW_CHARS{shortened=true;json!(s.chars().take(PREVIEW_CHARS).collect::<String>())}else{json!(s)}}}).collect::<Vec<_>>();json!([r,cells,false,false,types])}).collect::<Vec<_>>();
    let mut warnings=snapshot.warnings.clone();if shortened{warnings.push("Long cell values are shortened to 2000 characters in this preview; the project keeps the complete values.".into());}
    Ok(json!({"id":snapshot.id,"profileRevision":snapshot.prepared.profile_revision,"columns":t.names,"columnTypes":snapshot.types,"rows":rows,"total":t.rows,"warnings":warnings,"previewTruncated":shortened,"expiresInSeconds":TTL.saturating_sub(snapshot.created.elapsed()).as_secs()}))
}

pub fn test(params:J)->Result<J>{let(request,profile)=Request::parse(params)?;driver::run(&request,&profile,driver::Mode::Test)?;Ok(json!({"ok":true,"driver":profile.driver}))}
pub fn schema(params:J)->Result<J>{let(request,profile)=Request::parse(params)?;match driver::run(&request,&profile,driver::Mode::Schema)?{driver::Output::Schema(value)=>Ok(value),_=>unreachable!()}}
pub fn prepare(params:J)->Result<J>{
    let(request,profile)=Request::parse(params)?;let sql=read_query(&request.sql,&profile.driver)?;
    let driver::Output::Rows(rows)=driver::run(&request,&profile,driver::Mode::Query(&sql))?else{unreachable!()};
    jobs::tick(0)?;
    let snapshot=Snapshot{prepared:Prepared{table:rows.table,name:profile.name,profile_id:profile.id,profile_revision:profile.revision,driver:profile.driver},id:String::new(),types:rows.types,warnings:rows.warnings,bytes:rows.bytes,created:Instant::now()};
    let snapshot=cache().lock().unwrap_or_else(|e|e.into_inner()).insert(snapshot)?;view(&snapshot,0,20)
}

struct Rows {
    names:Vec<String>,types:Vec<String>,columns:Vec<(Interner,crate::spill::CodeBuilder)>,rows:usize,bytes:usize,max_rows:usize,binary:bool,
}
struct ResultRows {table:Table,types:Vec<String>,bytes:usize,warnings:Vec<String>}
impl Rows {
    fn new(columns:Vec<(String,String)>,max_rows:usize)->Result<Self>{
        if columns.is_empty()||columns.len()>512{return Err("Database query must return between 1 and 512 columns".into());}
        let(names,types):(Vec<_>,Vec<_>)=columns.into_iter().unzip();let width=names.len();let names=crate::import::column_names(names,width);
        let bytes=names.iter().chain(&types).map(String::len).sum::<usize>();
        let budget=crate::storage::current_budget();
        Ok(Self{names,types,columns:(0..width).map(|_|(Interner::default(),crate::spill::CodeBuilder::new(budget.clone()))).collect(),rows:0,bytes,max_rows,binary:false})
    }
    fn push(&mut self,values:Vec<Value>)->Result<()> {
        jobs::tick(1)?;if self.rows>=self.max_rows{return Err(format!("Query exceeds {} rows. Add an SQL LIMIT or increase maxRows; no partial snapshot was kept.",self.max_rows));}
        for ((dict,codes),value) in self.columns.iter_mut().zip(values){
            let bytes=match &value{Value::Str(s)=>s.len().saturating_mul(4).saturating_add(64),_=>32};
            self.bytes=self.bytes.checked_add(bytes).ok_or("Database result byte count overflow")?;
            if self.bytes>MAX_BYTES{return Err("Database result exceeds the 128 MiB metadata budget; narrow the query".into());}
            if matches!(&value,Value::Str(s)if s.starts_with("base64:")){self.binary=true;}
            codes.push(dict.intern(value))?;
        }
        self.rows+=1;Ok(())
    }
    fn finish(self)->Result<ResultRows> {
        let columns=self.columns.into_iter().map(|(dict,codes)|Ok(Column{dict:dict.finish(),codes:codes.finish()?,recon:None})).collect::<Result<Vec<_>>>()?;
        let mut warnings=vec!["Exact decimal values, dates and booleans remain text; SQL NULL and empty text remain distinct.".into()];
        if self.binary{warnings.push("Binary values are encoded as base64: followed by standard Base64; their bytes are preserved.".into());}
        Ok(ResultRows{table:Table::new(self.names,columns,self.rows),types:self.types,bytes:self.bytes,warnings})
    }
}
fn binary(bytes:Vec<u8>)->Value{use base64::Engine;Value::Str(format!("base64:{}",base64::engine::general_purpose::STANDARD.encode(bytes)))}
fn finite(number:f64)->Value{if number.is_finite(){Value::Num(number)}else{Value::Str(number.to_string())}}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn profiles_never_accept_or_serialize_passwords(){
        let secret="never-persist-this-password";let profile=json!({"name":"fixture","driver":"sqlite","sqlitePath":"fixture.sqlite","password":secret});
        assert!(!validate_profile(profile).err().unwrap().contains(secret));
        let p=validate_profile(json!({"name":"fixture","driver":"postgres","host":"localhost","database":"synthetic","username":"reader"})).unwrap();
        assert_eq!(p.port,5432);let saved=serde_json::to_value(p).unwrap();assert!(saved.get("password").is_none());assert_eq!(saved["tls"],"verify-full");
        for path in ["//server/database.sqlite","\\\\server\\database.sqlite","file:database.sqlite?mode=rw"]{assert!(validate_profile(json!({"name":"fixture","driver":"sqlite","sqlitePath":path})).is_err());}
    }
    #[test]
    fn read_query_cannot_hide_side_effects_inside_dialect_comments_or_identifiers(){
        for query in ["SELECT 1", "SELECT '; DELETE' AS literal; -- harmless", "WITH x AS (SELECT 1) SELECT * FROM x", "SELECT * FROM \"update\"", "SELECT * FROM `create`"]{assert!(read_query(query,"sqlite").is_ok(),"{query}");}
        for query in ["SELECT 1; DELETE FROM x","WITH x AS (DELETE FROM x RETURNING *) SELECT * FROM x","SELECT 1 INTO OUTFILE 'x'","SELECT 1--x INTO OUTFILE 'x'","SELECT 1 /*!50000 INTO OUTFILE 'x' */","SELECT 1 /*M! INTO OUTFILE 'x' */","SELECT 1 /*M!100100 INTO OUTFILE 'x' */","SELECT 1 /* outer /* inner */ INTO OUTFILE 'x' -- */","SELECT \"pg_cancel_backend\"(1)","SELECT `get_lock`('name',1)","SELECT @x:=1","SELECT $$text$$","SELECT ARRAY[pg_cancel_backend(123)]","SELECT ARRAY[ARRAY[\"lo_export\"(1,'x')]]"]{assert!(read_query(query,"postgres").is_err(),"{query}");}
        assert!(read_query("SELECT ARRAY[1,2,3]","postgres").is_ok());assert!(read_query("SELECT [update] FROM [table]","sqlite").is_ok());
    }
    #[test]
    fn local_staging_expires_and_clips_only_the_preview(){
        fn snapshot(text:&str)->Snapshot{let mut rows=Rows::new(vec![("text".into(),"TEXT".into())],1).unwrap();rows.push(vec![Value::Str(text.into())]).unwrap();let rows=rows.finish().unwrap();Snapshot{prepared:Prepared{table:rows.table,name:"Fixture".into(),profile_id:"id".into(),profile_revision:"revision".into(),driver:"sqlite".into()},id:String::new(),types:rows.types,warnings:rows.warnings,bytes:rows.bytes,created:Instant::now()}}
        let mut cache=Cache::default();let first=cache.insert(snapshot("one")).unwrap().id;
        for _ in 0..4{cache.insert(snapshot("two")).unwrap();}assert!(cache.get(&first).is_err());assert_eq!(cache.items.len(),4);
        let id=cache.items[0].id.clone();cache.items[0].created=Instant::now()-TTL-Duration::from_secs(1);assert!(cache.get(&id).is_err());
        let full="界".repeat(2010);let s=snapshot(&full);let preview=view(&s,0,20).unwrap();assert_eq!(preview["rows"][0][1][0].as_str().unwrap().chars().count(),2000);assert_eq!(s.prepared.table.columns[0].get(0).as_str(),Some(full.as_str()));
        let mut big=snapshot("large");big.bytes=MAX_BYTES+1;assert!(cache.insert(big).is_err());
    }
}
