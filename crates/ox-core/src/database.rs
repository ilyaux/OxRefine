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

/// Export → Database table: rows into a new PostgreSQL table whose name ends in `_ox`, the only
/// tables OxRefine writes. With `key`, only what changed since the import: that column, the columns
/// that differ from the imported values in some row, and those rows, ready for a correlated UPDATE.
/// `dryRun` describes the table without connecting.
pub fn export(params:J,table:&Table,rows:&[u32],original:&Table)->Result<J>{
    #[derive(Deserialize)]
    #[serde(rename_all="camelCase")]
    struct Target{table:String,#[serde(default)]replace:bool,#[serde(default)]key:Option<String>,#[serde(default)]dry_run:bool}
    let target:Target=serde_json::from_value(params.clone()).map_err(|_|"Database export needs a table name")?;
    let upload=Upload::plan(&target.table,target.replace,target.key.as_deref().filter(|k|!k.is_empty()),table,rows,original)?;
    if target.dry_run{return Ok(upload.describe());}
    let mut shared=params;if let Some(o)=shared.as_object_mut(){for k in["table","replace","key","dryRun"]{o.remove(k);}}
    let(request,profile)=Request::parse(shared)?;
    if profile.driver!="postgres"{return Err("Writing a table is available for PostgreSQL connections".into());}
    driver::run(&request,&profile,driver::Mode::Write(&upload))?;
    Ok(upload.describe())
}
pub(crate) struct Upload<'a>{name:String,target:String,replace:bool,table:&'a Table,rows:Vec<u32>,cols:Vec<usize>,kinds:Vec<&'static str>}
impl<'a> Upload<'a>{
    fn plan(name:&str,replace:bool,key:Option<&str>,table:&'a Table,rows:&[u32],original:&Table)->Result<Self>{
        let parts:Vec<&str>=name.split('.').collect();
        let word=|p:&&str|!p.is_empty()&&p.len()<=63&&p.bytes().next().is_some_and(|b|b.is_ascii_lowercase()||b==b'_')&&p.bytes().all(|b|b.is_ascii_lowercase()||b.is_ascii_digit()||b==b'_');
        let last=parts.last().copied().unwrap_or_default();
        if parts.len()>2||!parts.iter().all(word)||!last.ends_with("_ox")||last=="_ox"{return Err("The table name must end in _ox and use lowercase letters, digits and _, optionally after a schema (schema.name_ox). OxRefine writes no other tables.".into());}
        let target=parts.iter().map(|p|format!("\"{p}\"")).collect::<Vec<_>>().join(".");
        let(rows,cols)=match key{
            None=>{if rows.is_empty(){return Err("No rows match the facets, so no table was written.".into());}(rows.to_vec(),(0..table.names.len()).collect::<Vec<_>>())},
            Some(key)=>{
                let k=table.col(key)?;let ok=original.col(key).map_err(|_|format!("The key column {key} wasn't in the imported data; choose a column that came with it"))?;
                let id=|v:&Value|format!("{v:?}");
                let mut imported=std::collections::HashMap::new();
                for r in 0..original.rows{let v=original.columns[ok].get(r);if v.is_blank(){continue;}if imported.insert(id(v),r).is_some(){return Err(format!("The key {} appears more than once in the imported data; choose a column that identifies each row",v.display()));}}
                let compare:Vec<(usize,Option<usize>)>=(0..table.names.len()).filter(|&c|c!=k).map(|c|(c,original.col(&table.names[c]).ok())).collect();
                let(null,mut changed,mut keep,mut seen)=(Value::Null,vec![false;compare.len()],vec![],std::collections::HashSet::new());
                for &r in rows{
                    let v=table.columns[k].get(r as usize);
                    let o=*imported.get(&id(v)).ok_or_else(||format!("Row {} has the key {}, which isn't in the imported data; the key column must keep its imported values",r+1,v.display()))?;
                    if !seen.insert(o){return Err(format!("The key {} is on more than one row; the key column must identify each row",v.display()));}
                    let mut any=false;
                    for(i,&(c,oc))in compare.iter().enumerate(){if table.columns[c].get(r as usize)!=oc.map_or(&null,|oc|original.columns[oc].get(o)){changed[i]=true;any=true;}}
                    if any{keep.push(r);}
                }
                if keep.is_empty(){return Err("Nothing changed since the import in the rows matching the facets, so no table was written.".into());}
                (keep,std::iter::once(k).chain(compare.iter().zip(&changed).filter(|(_,changed)|**changed).map(|(&(c,_),_)|c)).collect())
            },
        };
        for &c in &cols{if table.names[c].len()>63||table.names[c].contains('\0'){return Err(format!("The column name {} is longer than PostgreSQL's 63 bytes; rename the column first",table.names[c]));}}
        // one type a column: whole numbers bigint, numbers double precision, booleans, dates, the rest text
        let kinds=cols.iter().map(|&c|rows.iter().map(|&r|table.columns[c].get(r as usize)).fold(None,|kind,v|{
            let this=match v{Value::Null=>return kind,Value::Int(_)=>"bigint",Value::Num(_)=>"double precision",Value::Bool(_)=>"boolean",Value::Date(_)=>"timestamptz",_=>"text"};
            Some(match kind{None=>this,Some(k)if k==this=>k,Some("bigint"|"double precision")if matches!(this,"bigint"|"double precision")=>"double precision",_=>"text"})
        }).unwrap_or("text")).collect();
        Ok(Upload{name:name.into(),target,replace,table,rows,cols,kinds})
    }
    fn describe(&self)->J{
        let cell=|v:&Value|match v{Value::Null=>J::Null,Value::Int(n)=>json!(n),Value::Num(n)=>json!(n),Value::Bool(b)=>json!(b),v=>json!(v.display())};
        json!({"table":self.name,"rows":self.rows.len(),
            "columns":self.cols.iter().zip(&self.kinds).map(|(&c,k)|json!({"name":self.table.names[c],"type":k})).collect::<Vec<_>>(),
            "sample":self.rows.iter().take(10).map(|&r|self.cols.iter().map(|&c|cell(self.table.columns[c].get(r as usize))).collect::<Vec<_>>()).collect::<Vec<_>>()})
    }
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
    fn table(names:&[&str],rows:Vec<Vec<Value>>)->Table{let mut t=Rows::new(names.iter().map(|n|(n.to_string(),String::new())).collect(),100).unwrap();for r in rows{t.push(r).unwrap();}t.finish().unwrap().table}
    #[test]
    fn database_export_writes_only_ox_tables_and_with_a_key_only_the_changes(){
        use Value::*;let s=|v:&str|Str(v.into());
        let original=table(&["id","name","city"],vec![vec![Int(1),s("Ann"),s("Rome")],vec![Int(2),s("Bob"),s("Oslo")],vec![Int(3),s("Cy"),Null]]);
        let plan=|name:&str,key:Option<&str>,t:&Table,rows:&[u32]|Upload::plan(name,false,key,t,rows,&original).map(|u|u.describe());
        for bad in["people","People_ox","_ox","a.b.c_ox","x_ox\"; DROP TABLE people; --","x_OX","1x_ox"]{assert!(plan(bad,None,&original,&[0]).is_err(),"{bad}");}
        let whole=plan("public.people_ox",None,&original,&[0,2]).unwrap();
        assert_eq!(whole["rows"],2);assert_eq!(whole["columns"],json!([{"name":"id","type":"bigint"},{"name":"name","type":"text"},{"name":"city","type":"text"}]));assert_eq!(whole["sample"][1],json!([3,"Cy",null]));
        assert!(plan("people_ox",None,&original,&[]).unwrap_err().contains("No rows"));
        // Bob renamed, a new column filled for Cy: the key, those two columns and those two rows
        let now=table(&["id","name","city","score"],vec![vec![Int(1),s("Ann"),s("Rome"),Null],vec![Int(2),s("Robert"),s("Oslo"),Null],vec![Int(3),s("Cy"),Null,Num(0.5)]]);
        let changes=plan("people_ox",Some("id"),&now,&[0,1,2]).unwrap();
        assert_eq!(changes["columns"],json!([{"name":"id","type":"bigint"},{"name":"name","type":"text"},{"name":"score","type":"double precision"}]));
        assert_eq!(changes["sample"],json!([[2,"Robert",null],[3,"Cy",0.5]]));
        assert!(plan("people_ox",Some("id"),&now,&[0]).unwrap_err().contains("Nothing changed"),"facets narrow the changes");
        assert!(plan("people_ox",Some("score"),&now,&[0]).unwrap_err().contains("wasn't in the imported data"));
        let moved=table(&["id","name"],vec![vec![Int(9),s("Ann")]]);assert!(plan("people_ox",Some("id"),&moved,&[0]).unwrap_err().contains("must keep its imported values"));
        let twice=table(&["id","name"],vec![vec![Int(1),s("A")],vec![Int(1),s("B")]]);assert!(plan("people_ox",Some("id"),&twice,&[0,1]).unwrap_err().contains("more than one row"));
        // a value that is now text, and blank text, keep the column text; Int and "1" are different keys
        let mixed=table(&["id","name","city"],vec![vec![Int(1),s("Ann"),Int(5)],vec![Int(2),s("Bob"),s("")]]);
        assert_eq!(plan("people_ox",Some("id"),&mixed,&[0,1]).unwrap()["columns"][1],json!({"name":"city","type":"text"}));
        let text_keys=table(&["id","name"],vec![vec![s("1"),s("Ann")]]);assert!(plan("people_ox",Some("id"),&text_keys,&[0]).is_err());
        assert!(export(json!({"table":"people_ox","profile":{"name":"x","driver":"sqlite","sqlitePath":"x.sqlite"},"requestId":"r"}),&original,&[0],&original).unwrap_err().contains("PostgreSQL"));
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
