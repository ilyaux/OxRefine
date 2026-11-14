//! Anonymous Commons metadata imports. The only production endpoint is Wikimedia's Action API.
//! A complete, bounded snapshot is staged before project creation; no network operation saves data.
use crate::{err, grel::Value, jobs, recon::{Candidate, Judgment, Recon, ReconCol}, table::{Column, Table}, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as J};
use std::{collections::{HashMap, HashSet}, sync::{Arc, Mutex, OnceLock}, time::{Duration, Instant}};

const API: &str = "https://commons.wikimedia.org/w/api.php";
const SERVICE: &str = "https://commonsreconcile.toolforge.org/en/api";
const SPACE: &str = "https://commons.wikimedia.org/entity/";
const SCHEMA: &str = "http://www.wikidata.org/prop/direct/";
const MAX_BYTES: usize = 128 * 1024 * 1024;
const MAX_RESPONSE: u64 = 8 * 1024 * 1024;
const MAX_CATEGORIES: usize = 1000;
const MAX_REQUESTS: usize = 2000;
const TTL: Duration = Duration::from_secs(1800);
const PREVIEW_CHARS: usize = 2000;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Category { pub name: String, #[serde(default)] pub depth: u8 }

#[derive(Clone, Deserialize)]
#[serde(rename_all="camelCase", default, deny_unknown_fields)]
struct Options { categories: Vec<Category>, include_wikitext: bool, max_files: usize, request_id: String }
impl Default for Options {
    fn default() -> Self { Self { categories:vec![],include_wikitext:false,max_files:10_000,request_id:String::new() } }
}
impl Options {
    fn parse(value: J) -> Result<Self> {
        let mut options:Self=serde_json::from_value(value).map_err(err)?;
        if options.categories.is_empty() || options.categories.len()>20 { return Err("Enter between 1 and 20 Commons categories".into()); }
        if !(1..=50_000).contains(&options.max_files) { return Err("maxFiles must be between 1 and 50000".into()); }
        if options.request_id.is_empty() || options.request_id.len()>64 || !options.request_id.bytes().all(|b|b.is_ascii_alphanumeric()||matches!(b,b'-'|b'_')) { return Err("Commons import needs a requestId of 1–64 letters, digits, hyphens or underscores".into()); }
        for c in &mut options.categories {
            if c.depth>10 { return Err("Commons category depth must be between 0 and 10".into()); }
            let title=c.name.trim().replace('_'," ");
            let name=if title.get(..9).is_some_and(|p|p.eq_ignore_ascii_case("Category:")) { title[9..].trim() } else { title.as_str() };
            if name.is_empty() || name.len()>512 || name.contains("://") || name.chars().any(|c|c.is_control()||matches!(c,'|'|'['|']'|'{'|'}'|'#')) { return Err("Use a Commons category name, with an optional Category: prefix; URLs and API endpoints are not accepted".into()); }
            c.name=format!("Category:{name}");
        }
        Ok(options)
    }
}

#[derive(Clone)]
pub struct Prepared { pub table: Table, pub source_url: String, pub name: String, pub categories: Vec<Category> }
#[derive(Clone, Default)]
struct Counts { categories: usize, requests: usize, duplicates: usize, revisits: usize }
#[derive(Clone)]
struct Snapshot { prepared: Prepared, request_id: String, counts: Counts, bytes: usize, created: Instant, id: String }
#[derive(Default)]
struct Cache { snapshots: Vec<Snapshot> }
impl Cache {
    fn prune(&mut self) { self.snapshots.retain(|s|s.created.elapsed()<TTL); }
    fn insert(&mut self, mut snapshot: Snapshot) -> Result<Snapshot> {
        self.prune();
        if snapshot.bytes>MAX_BYTES { return Err("Commons snapshot exceeds the 128 MiB metadata budget; use fewer files or disable wikitext".into()); }
        while !self.snapshots.is_empty() && (self.snapshots.len()>=4 || self.snapshots.iter().map(|s|s.bytes).sum::<usize>()+snapshot.bytes>MAX_BYTES) { self.snapshots.remove(0); }
        snapshot.id=format!("{:016x}{:016x}",crate::recon::new_id() as u64,crate::recon::new_id() as u64);
        self.snapshots.push(snapshot.clone());Ok(snapshot)
    }
    fn get(&mut self,id:&str)->Result<Snapshot> { self.prune();self.snapshots.iter().find(|s|s.id==id).cloned().ok_or_else(||"This Commons preview expired, was evicted or belongs to an earlier server run. Prepare it again before creating the project.".into()) }
}
fn cache()->&'static Mutex<Cache> { static CACHE:OnceLock<Mutex<Cache>>=OnceLock::new();CACHE.get_or_init(||Mutex::new(Cache::default())) }

/// Fetch all requested metadata, then atomically publish an immutable preview handle.
/// The host runs this as an owned job, outside its project/session lock.
pub fn prepare(params:J)->Result<J> {
    let options=Options::parse(params)?;
    jobs::progress(&format!("Commons request {}",options.request_id),0);jobs::tick(0)?;
    let snapshot=fetch(&Client::new(),&options)?;
    jobs::tick(0)?;
    let snapshot=cache().lock().unwrap_or_else(|e|e.into_inner()).insert(snapshot)?;
    preview_snapshot(&snapshot,0,20)
}
pub fn preview(id:&str,start:usize,limit:usize)->Result<J> {
    let snapshot=cache().lock().unwrap_or_else(|e|e.into_inner()).get(id)?;
    preview_snapshot(&snapshot,start,limit)
}
pub fn prepared(id:&str)->Result<Prepared> { Ok(cache().lock().unwrap_or_else(|e|e.into_inner()).get(id)?.prepared) }
fn preview_snapshot(snapshot:&Snapshot,start:usize,limit:usize)->Result<J> {
    if limit>100 { return Err("Commons preview is limited to 100 rows per page".into()); }
    let limit=if limit==0 {20}else{limit};let t=&snapshot.prepared.table;let mut shortened=false;
    let rows=(start.min(t.rows)..t.rows).take(limit).map(|r| {
        let cells=t.columns.iter().map(|c| match c.get(r) { Value::Str(s) if s.chars().count()>PREVIEW_CHARS=>{shortened=true;json!(s.chars().take(PREVIEW_CHARS).collect::<String>())},Value::Str(s)=>json!(s),_=>J::Null }).collect::<Vec<_>>();
        let recon=t.columns.iter().enumerate().filter_map(|(ci,c)|c.recon(r).map(|recon|(ci.to_string(),crate::recon::brief(recon)))).collect::<serde_json::Map<_,_>>();
        json!([r,cells,false,false,"s".repeat(t.columns.len()),null,recon])
    }).collect::<Vec<_>>();
    let mut warnings=vec![];
    if t.rows==0 { warnings.push("No files were found within the requested category depth."); }
    if shortened {warnings.push("Wikitext preview is shortened to 2000 characters per file. The project keeps the full retrieved text.");}
    Ok(json!({"id":snapshot.id,"requestId":snapshot.request_id,"columns":t.names,"rows":rows,"total":t.rows,"categories":snapshot.prepared.categories,"visitedCategories":snapshot.counts.categories,"requests":snapshot.counts.requests,"duplicatesSkipped":snapshot.counts.duplicates,"categoryRevisits":snapshot.counts.revisits,"warnings":warnings,"previewTruncated":shortened,"expiresInSeconds":TTL.saturating_sub(snapshot.created.elapsed()).as_secs()}))
}

struct Client { endpoint:String, agent:ureq::Agent, throttle:Duration }
impl Client {
    fn new()->Self {
        Self {endpoint:API.into(),throttle:Duration::from_millis(100),agent:ureq::Agent::new_with_config(ureq::Agent::config_builder().http_status_as_error(false).max_redirects(0)
            .timeout_connect(Some(Duration::from_secs(5))).timeout_global(Some(Duration::from_secs(20)))
            .user_agent(concat!("OxRefine/",env!("CARGO_PKG_VERSION")," (+https://github.com/ilyaux/OxRefine; Commons metadata import)")).build())}
    }
    fn query(&self,pairs:&[(&str,String)],counts:&mut Counts)->Result<J> {
        let mut url=url::Url::parse(&self.endpoint).map_err(err)?;
        url.query_pairs_mut().extend_pairs([("action","query"),("format","json"),("formatversion","2"),("maxlag","5")]).extend_pairs(pairs.iter().map(|(k,v)|(*k,v.as_str())));
        for attempt in 0..3 {
            jobs::tick(0)?;
            if counts.requests>=MAX_REQUESTS { return Err("Commons import reached the 2000-request budget; use a smaller category or depth".into()); }
            // Serialize metadata requests across imports too. Waiters remain cancellable.
            static GATE:Mutex<Option<Instant>>=Mutex::new(None);
            let mut gate=loop { match GATE.try_lock(){Ok(g)=>break g,Err(std::sync::TryLockError::Poisoned(e))=>break e.into_inner(),Err(_)=>pause(Duration::from_millis(50))?} };
            if let Some(last)=*gate {pause(self.throttle.saturating_sub(last.elapsed()))?;}
            *gate=Some(Instant::now());counts.requests+=1;
            let response=self.agent.get(url.as_str()).header("Accept","application/json").call();
            let mut response=match response {Ok(r)=>r,Err(error)=>{drop(gate);jobs::tick(0)?;return Err(format!("Commons metadata request failed: {error}"));}};
            let status=response.status().as_u16();
            let retry=response.headers().get("Retry-After").and_then(|h|h.to_str().ok()).map(str::to_owned);
            let text=response.body_mut().with_config().limit(MAX_RESPONSE).read_to_string().map_err(|e|format!("Commons response could not be read within the 8 MiB/20-second request limits: {e}"));
            drop(gate);jobs::tick(0)?;
            let text=text?;
            let data=serde_json::from_str::<J>(&text);
            let code=data.as_ref().ok().and_then(|v|v["error"]["code"].as_str()).unwrap_or("");
            let transient=matches!(status,429|503)||matches!(code,"maxlag"|"ratelimited");
            if transient && attempt<2 {
                let seconds=if let Some(header)=retry.as_deref() {
                    if let Ok(seconds)=header.parse::<u64>(){seconds}
                    else {let when=chrono::DateTime::parse_from_rfc2822(header).map_err(|_|"Commons sent an unreadable Retry-After header; retry the import later")?;let millis=(when.timestamp_millis()-chrono::Utc::now().timestamp_millis()).max(0)as u64;millis.div_ceil(1000)}
                }else if code=="maxlag"{5}else{1<<attempt};
                if seconds>30 {return Err(format!("Commons is busy and asks for a {seconds}-second retry delay. Try preparing the import later."));}
                jobs::progress("Waiting for Commons API backoff",0);pause(Duration::from_secs(seconds))?;continue;
            }
            if status!=200 {return Err(format!("Commons returned HTTP {status}; no project was created. Try again later for rate/server errors."));}
            let data=data.map_err(|e|format!("Commons returned invalid JSON: {e}"))?;
            if let Some(error)=data.get("error") {return Err(format!("Commons API error {}: {}",error["code"].as_str().unwrap_or("unknown"),error["info"].as_str().unwrap_or("request failed").chars().take(500).collect::<String>()));}
            if data.get("warnings").is_some(){return Err(format!("Commons returned an API warning; the import was stopped rather than using an incomplete query: {}",data["warnings"].to_string().chars().take(500).collect::<String>()));}
            return Ok(data);
        }
        Err("Commons remained rate limited after three attempts; try again later".into())
    }
}
fn pause(duration:Duration)->Result<()> {let start=Instant::now();while start.elapsed()<duration{jobs::tick(0)?;std::thread::sleep(duration.saturating_sub(start.elapsed()).min(Duration::from_millis(50)));}jobs::tick(0)}
fn continuation(data:&J,kind:&str,seen:&mut HashSet<String>)->Result<Vec<(&'static str,String)>> {
    let Some(next)=data.get("continue") else{return Ok(vec![])};
    let next=next.as_object().ok_or("Invalid Commons continuation object")?;
    if next.is_empty(){return Err("Commons returned an empty continuation object".into());}
    let mut result=vec![];
    for (key,value) in next {
        let key=match key.as_str(){"continue"=>"continue","cmcontinue" if kind=="category"=>"cmcontinue","rvcontinue" if kind=="revisions"=>"rvcontinue",_=>return Err("Unexpected Commons continuation parameter".into())};
        let value=value.as_str().filter(|s|s.len()<=4096).ok_or("Invalid Commons continuation token")?;result.push((key,value.to_string()));
    }
    if !seen.insert(serde_json::to_string(next).map_err(err)?){return Err("Commons repeated a continuation token; no project was created".into());}Ok(result)
}

#[derive(Clone)]
struct File { id:u64,title:String,wikitext:Option<String> }
#[derive(Clone)]
struct Members { files:Vec<File>,categories:Vec<String> }
struct Fetch<'a> { client:&'a Client,options:&'a Options,counts:Counts,cache:HashMap<String,Arc<Members>>,visited:HashMap<String,u8>,files:Vec<File>,ids:HashSet<u64>,bytes:usize }
impl Fetch<'_> {
    fn budget(&mut self,bytes:usize)->Result<()> {self.bytes=self.bytes.checked_add(bytes).ok_or("Commons metadata byte count overflow")?;if self.bytes>MAX_BYTES{return Err("Commons import exceeds the 128 MiB metadata budget; reduce depth/files or disable wikitext".into());}Ok(())}
    fn members(&mut self,title:&str)->Result<Arc<Members>> {
        if let Some(m)=self.cache.get(title){return Ok(m.clone());}
        if self.counts.categories>=MAX_CATEGORIES{return Err("Commons import reached the 1000-category budget; reduce depth or roots".into());}
        self.counts.categories+=1;jobs::progress(&format!("Reading Commons {title}"),0);
        let mut members=Members{files:vec![],categories:vec![]};let mut next=vec![];let mut seen=HashSet::new();let mut new_ids=HashSet::new();
        loop {
            let mut query=vec![("list","categorymembers".into()),("cmtitle",title.into()),("cmtype","file|subcat".into()),("cmprop","ids|title|type".into()),("cmlimit","500".into())];query.extend(next);
            let data=self.client.query(&query,&mut self.counts)?;
            let batch=data["query"]["categorymembers"].as_array().ok_or("Commons response omitted category members")?;
            for item in batch {
                jobs::tick(0)?;
                let title=item["title"].as_str().filter(|s|s.len()<=2048).ok_or("Invalid Commons member title")?.to_string();
                match item["ns"].as_u64() {
                    Some(6)=>{
                        let id=item["pageid"].as_u64().filter(|&id|id>0).ok_or("A Commons file has no positive page id")?;
                        if !self.ids.contains(&id) && new_ids.insert(id) && self.files.len()+new_ids.len()>self.options.max_files{return Err(format!("Commons contains more than {} files at this depth. Choose fewer files/categories or reduce depth; no partial project was created.",self.options.max_files));}
                        self.budget(title.len()+128)?;members.files.push(File{id,title,wikitext:None});
                    }
                    Some(14)=>{self.budget(title.len()+64)?;members.categories.push(title);}
                    _=>{},
                }
            }
            next=continuation(&data,"category",&mut seen)?;if next.is_empty(){break;}
        }
        let members=Arc::new(members);self.cache.insert(title.into(),members.clone());Ok(members)
    }
    fn walk(&mut self,title:&str,depth:u8)->Result<()> {
        jobs::tick(0)?;
        if self.visited.get(title).is_some_and(|&old|old>=depth){self.counts.revisits+=1;return Ok(());}
        if self.visited.insert(title.into(),depth).is_some(){self.counts.revisits+=1;}
        let members=self.members(title)?;
        // Source order: direct files first, then subcategories depth-first. Unlike the extension,
        // overlapping roots/cycles cannot duplicate rows or cause repeated HTTP pagination.
        for file in &members.files {
            if self.ids.insert(file.id) {
                if self.files.len()>=self.options.max_files{return Err(format!("Commons contains more than {} files at this depth. Choose fewer files/categories or reduce depth; no partial project was created.",self.options.max_files));}
                self.budget(file.title.len()*4+2048)?;self.files.push(file.clone());
            }else{self.counts.duplicates+=1;}
        }
        if depth>0 {for category in &members.categories{self.walk(category,depth-1)?;}}
        Ok(())
    }
    fn wikitext(&mut self)->Result<()> {
        jobs::progress_units("Reading Commons file-page wikitext",self.files.len()as u64,"files");
        for start in (0..self.files.len()).step_by(50) {
            let end=(start+50).min(self.files.len());let ids=self.files[start..end].iter().map(|f|f.id.to_string()).collect::<Vec<_>>().join("|");
            let requested=self.files[start..end].iter().map(|f|f.id).collect::<HashSet<_>>();
            let mut next=vec![];let mut seen=HashSet::new();let mut found=HashMap::new();
            loop {
                let mut query=vec![("pageids",ids.clone()),("prop","revisions".into()),("rvprop","content".into()),("rvslots","main".into())];query.extend(next);
                let data=self.client.query(&query,&mut self.counts)?;
                for page in data["query"]["pages"].as_array().ok_or("Commons response omitted revision pages")? {
                    let id=page["pageid"].as_u64().ok_or("A Commons revision page has no id")?;
                    if !requested.contains(&id){return Err(format!("Commons returned unrequested revision page {id}; no project was created"));}
                    // A property continuation may list a completed page without its revisions.
                    if found.contains_key(&id){continue;}
                    if page["missing"].as_bool()==Some(true){return Err(format!("Commons file page {id} disappeared while importing; prepare again"));}
                    let revision=&page["revisions"][0];let content=revision["slots"]["main"]["content"].as_str().or_else(||revision["slots"]["main"]["*"].as_str()).or_else(||revision["*"].as_str()).ok_or_else(||format!("Commons did not return readable wikitext for page {id}"))?;
                    if !found.contains_key(&id){self.budget(content.len())?;found.insert(id,content.to_string());}
                }
                next=continuation(&data,"revisions",&mut seen)?;if next.is_empty(){break;}
            }
            for file in &mut self.files[start..end]{file.wikitext=Some(found.remove(&file.id).ok_or_else(||format!("Commons omitted wikitext for {}",file.title))?);jobs::tick(1)?;}
        }
        Ok(())
    }
}
fn fetch(client:&Client,options:&Options)->Result<Snapshot> {
    let mut fetch=Fetch{client,options,counts:Counts::default(),cache:HashMap::new(),visited:HashMap::new(),files:vec![],ids:HashSet::new(),bytes:0};
    for c in &options.categories{fetch.walk(&c.name,c.depth)?;}
    if options.include_wikitext{fetch.wikitext()?;}
    jobs::progress("Preparing Commons preview",fetch.files.len()as u64);
    let table=table(&fetch.files,options.include_wikitext)?;jobs::tick(fetch.files.len()as u64)?;
    let mut url=url::Url::parse("https://commons.wikimedia.org/wiki/").map_err(err)?;url.path_segments_mut().map_err(|_|"Invalid Commons source URL")?.pop_if_empty().push(&options.categories[0].name);
    let name=if options.categories.len()==1{options.categories[0].name.clone()}else{format!("{} (+{} categories)",options.categories[0].name,options.categories.len()-1)};
    Ok(Snapshot{prepared:Prepared{table,source_url:url.to_string(),name,categories:options.categories.clone()},request_id:options.request_id.clone(),counts:fetch.counts,bytes:fetch.bytes,created:Instant::now(),id:String::new()})
}
fn table(files:&[File],wikitext:bool)->Result<Table> {
    let mut column=Column::from_values(files.iter().map(|f|Value::Str(f.title.clone())));
    let cells=files.iter().map(|file| {
        let candidate=Candidate{id:format!("M{}",file.id),name:file.title.clone(),types:vec![],score:100.0};
        let mut recon=Recon::new(0,SPACE,SCHEMA);recon.service=SERVICE.into();recon.candidates=Some(vec![candidate.clone()]);recon.matched=Some(candidate);recon.judgment=Judgment::Matched;recon.judgment_action="mass".into();recon.batch_size=1;
        Some(Arc::new(recon))
    }).collect();
    column.recon=Some(Arc::new(ReconCol{cells,config:Some(json!({"mode":"standard-service","service":SERVICE,"identifierSpace":SPACE,"schemaSpace":SCHEMA,"type":{"id":"","name":"entity"},"autoMatch":true,"columnDetails":[],"limit":1})),stats:None}));
    let mut names=vec!["File".into()];let mut columns=vec![column];
    if wikitext{names.push("Wikitext".into());columns.push(Column::from_values(files.iter().map(|f|f.wikitext.as_ref().map_or(Value::Null,|s|Value::Str(s.clone())))));}
    Ok(Table::new(names,columns,files.len()))
}

#[cfg(test)]
#[path="commons_tests.rs"]
mod tests;
