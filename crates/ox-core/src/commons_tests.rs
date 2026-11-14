use super::*;
use std::{io::{BufRead,BufReader,Write},net::TcpListener,sync::{atomic::{AtomicBool,Ordering},mpsc},thread};

struct Reply { status:u16, body:String, retry:Option<String> }
impl From<J> for Reply {fn from(value:J)->Self{Self{status:200,body:value.to_string(),retry:None}}}
#[derive(Clone)]
struct Request {query:HashMap<String,String>,headers:String}
struct Mock {url:String,seen:Arc<Mutex<Vec<Request>>>,stop:Arc<AtomicBool>,thread:Option<thread::JoinHandle<()>>}
impl Mock {
    fn new(handler:impl Fn(&Request)->Reply+Send+'static)->Self {
        let listener=TcpListener::bind("127.0.0.1:0").unwrap();listener.set_nonblocking(true).unwrap();let addr=listener.local_addr().unwrap();
        let stop=Arc::new(AtomicBool::new(false));let seen=Arc::new(Mutex::new(vec![]));let (stopped,requests)=(stop.clone(),seen.clone());
        let thread=thread::spawn(move||while !stopped.load(Ordering::Relaxed){
            match listener.accept(){Ok((stream,_))=>{
                // Windows accepted sockets can inherit the listener's nonblocking mode.
                stream.set_nonblocking(false).unwrap();
                stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();let mut reader=BufReader::new(stream);let mut first=String::new();if reader.read_line(&mut first).unwrap_or(0)==0{continue;}
                let mut headers=String::new();loop{let mut line=String::new();if reader.read_line(&mut line).unwrap_or(0)==0||line=="\r\n"{break;}headers+=&line;}
                let target=first.split_whitespace().nth(1).unwrap();let url=url::Url::parse(&format!("http://local{target}")).unwrap();
                let request=Request{query:url.query_pairs().map(|(k,v)|(k.into_owned(),v.into_owned())).collect(),headers};
                requests.lock().unwrap().push(request.clone());let response=handler(&request);let retry=response.retry.map(|r|format!("Retry-After: {r}\r\n")).unwrap_or_default();
                let _=write!(reader.get_mut(),"HTTP/1.1 {} Reply\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{retry}Connection: close\r\n\r\n{}",response.status,response.body.len(),response.body);
            },Err(e)if e.kind()==std::io::ErrorKind::WouldBlock=>thread::sleep(Duration::from_millis(5)),Err(_)=>break}
        });
        Self{url:format!("http://{addr}/api.php"),seen,stop,thread:Some(thread)}
    }
    fn client(&self)->Client{Client{endpoint:self.url.clone(),throttle:Duration::ZERO,..Client::new()}}
}
impl Drop for Mock {fn drop(&mut self){self.stop.store(true,Ordering::Relaxed);if let Some(thread)=self.thread.take(){let _=thread.join();}}}
fn options(depth:u8,wikitext:bool)->Options{Options::parse(json!({"categories":[{"name":"Root","depth":depth}],"includeWikitext":wikitext,"requestId":"commons-test"})).unwrap()}
fn file(id:u64,title:&str)->J{json!({"pageid":id,"ns":6,"title":title,"type":"file"})}
fn category(title:&str)->J{json!({"pageid":900,"ns":14,"title":title,"type":"subcat"})}
fn batch(members:Vec<J>,next:Option<&str>)->Reply{let mut value=json!({"query":{"categorymembers":members}});if let Some(next)=next{value["continue"]=json!({"continue":"-||","cmcontinue":next});}value.into()}
fn simple_snapshot()->Snapshot{
    let files=vec![File{id:123,title:"File:One.jpg".into(),wikitext:None}];
    Snapshot{prepared:Prepared{table:table(&files,false).unwrap(),source_url:"https://commons.wikimedia.org/wiki/Category:Root".into(),name:"Commons root".into(),categories:options(0,false).categories},request_id:"test".into(),counts:Counts::default(),bytes:2048,created:Instant::now(),id:String::new()}
}

#[test]
fn filename_and_full_reconciliation_match_executed_official_extension_mapping(){
    fn scores(value:&mut J){match value{J::Object(object)=>for(key,value)in object{if key=="score"&&value.is_number(){*value=json!(value.as_f64().unwrap());}else{scores(value)}},J::Array(items)=>for value in items{scores(value)},_=>{}}}
    let fixture:J=serde_json::from_str(include_str!("../../../tests/commons/reference.json")).unwrap();let mut checked=0;
    for case in fixture["cases"].as_array().unwrap(){
        if case["input"]["categoriesColumn"]!=false||case["input"]["mIdsColumn"]!=false{continue;}
        let files=case["input"]["files"].as_array().unwrap().iter().map(|f|File{id:f["pageId"].as_str().unwrap().parse().unwrap(),title:f["fileName"].as_str().unwrap().into(),wikitext:None}).collect::<Vec<_>>();
        let table=table(&files,false).unwrap();let rows=(0..table.rows).map(|row|{
            let mut recon=table.columns[0].recon(row).unwrap().to_json(true);recon.as_object_mut().unwrap().remove("id");
            json!([{ "v":table.columns[0].get(row).as_str().unwrap(),"recon":recon }])
        }).collect::<Vec<_>>();let mut expected=case["expected"]["rows"].clone();scores(&mut expected);assert_eq!(json!(rows),expected,"{}",case["name"]);checked+=1;
    }assert_eq!(checked,3);
}

#[test]
fn recursive_import_preserves_direct_first_order_continuations_and_limits_duplicates(){
    let mock=Mock::new(|r|match(r.query["cmtitle"].as_str(),r.query.get("cmcontinue").map(String::as_str)){
        ("Category:Root",None)=>batch(vec![file(1,"File:A.jpg"),category("Category:Child")],Some("p1")),
        ("Category:Root",Some("p1"))=>batch(vec![],Some("p2")),
        ("Category:Root",Some("p2"))=>batch(vec![file(2,"File:B.jpg")],None),
        ("Category:Child",None)=>batch(vec![file(3,"File:C.jpg"),file(1,"File:A.jpg"),category("Category:Root")],None),
        other=>panic!("Unexpected query {other:?}"),
    });
    let mut o=options(2,false);o.categories.push(Category{name:"Category:Child".into(),depth:0});let snapshot=fetch(&mock.client(),&o).unwrap();
    assert_eq!(snapshot.prepared.source_url,"https://commons.wikimedia.org/wiki/Category:Root");
    assert_eq!(snapshot.prepared.table.rows,3);assert_eq!((0..3).map(|r|snapshot.prepared.table.columns[0].get(r).as_str().unwrap()).collect::<Vec<_>>(),vec!["File:A.jpg","File:B.jpg","File:C.jpg"]);
    assert_eq!(snapshot.counts.categories,2);assert_eq!(snapshot.counts.requests,4);assert_eq!(snapshot.counts.duplicates,1);assert_eq!(snapshot.counts.revisits,2);
    for request in mock.seen.lock().unwrap().iter(){assert_eq!(request.query["action"],"query");assert_eq!(request.query["maxlag"],"5");assert!(request.headers.contains("OxRefine/"));assert!(request.headers.contains("Accept: application/json")||request.headers.contains("accept: application/json"));assert!(!request.headers.to_ascii_lowercase().contains("authorization:"));assert_eq!(request.query["cmtype"],"file|subcat");}
    let shallow=fetch(&mock.client(),&options(0,false)).unwrap();assert_eq!(shallow.prepared.table.rows,2);assert_eq!(shallow.counts.categories,1);
    let encoded=Mock::new(|r|{assert_eq!(r.query["cmtitle"],"Category:Café & birds/blue");batch(vec![file(1,"File:A.jpg")],None)});
    let o=Options::parse(json!({"requestId":"encoded-category","categories":[{"name":"Café & birds/blue"}]})).unwrap();
    assert_eq!(fetch(&encoded.client(),&o).unwrap().prepared.source_url,"https://commons.wikimedia.org/wiki/Category:Caf%C3%A9%20&%20birds%2Fblue");
}

#[test]
fn a_later_deeper_root_reuses_category_cache_and_expands_its_children(){
    let mock=Mock::new(|r|match r.query["cmtitle"].as_str(){"Category:Root"=>batch(vec![file(1,"File:A.jpg"),category("Category:Child")],None),"Category:Child"=>batch(vec![file(2,"File:B.jpg")],None),_=>panic!()});
    let mut o=options(0,false);o.categories.push(Category{name:"Category:Root".into(),depth:1});let snapshot=fetch(&mock.client(),&o).unwrap();
    assert_eq!(snapshot.prepared.table.rows,2);assert_eq!(snapshot.counts.requests,2);assert_eq!(snapshot.counts.revisits,1);assert_eq!(snapshot.counts.duplicates,1);
}

#[test]
fn wikitext_property_pagination_accepts_completed_pages_but_never_unrequested_ids(){
    let mock=Mock::new(|r|{
        if r.query.get("list").is_some(){return batch(vec![file(1,"File:A.jpg"),file(2,"File:B.jpg")],None);}
        assert_eq!(r.query["rvslots"],"main");assert_eq!(r.query["prop"],"revisions");
        if r.query.get("rvcontinue").is_none(){json!({"query":{"pages":[{"pageid":1,"revisions":[{"slots":{"main":{"content":"{{one}}"}}}]}]},"continue":{"continue":"||","rvcontinue":"next"}}).into()}
        else{json!({"query":{"pages":[{"pageid":1},{"pageid":2,"revisions":[{"slots":{"main":{"content":"[[two]]"}}}]}]}}).into()}
    });
    let snapshot=fetch(&mock.client(),&options(0,true)).unwrap();assert_eq!(snapshot.prepared.table.names,vec!["File","Wikitext"]);assert_eq!(snapshot.prepared.table.columns[1].get(0).as_str(),Some("{{one}}"));assert_eq!(snapshot.prepared.table.columns[1].get(1).as_str(),Some("[[two]]"));
    let wrong=Mock::new(|r|if r.query.get("list").is_some(){batch(vec![file(1,"File:A.jpg")],None)}else{json!({"query":{"pages":[{"pageid":99,"revisions":[{"slots":{"main":{"content":"wrong file"}}}]}]}}).into()});
    assert!(fetch(&wrong.client(),&options(0,true)).err().unwrap().contains("unrequested"));
}

#[test]
fn api_failures_bad_continuations_and_budget_exhaustion_fail_the_entire_snapshot(){
    let repeated=Mock::new(|_|batch(vec![],Some("same")));assert!(fetch(&repeated.client(),&options(0,false)).err().unwrap().contains("repeated"));assert_eq!(repeated.seen.lock().unwrap().len(),2);
    let injected=Mock::new(|_|json!({"query":{"categorymembers":[]},"continue":{"action":"delete"}}).into());assert!(fetch(&injected.client(),&options(0,false)).err().unwrap().contains("Unexpected"));assert_eq!(injected.seen.lock().unwrap().len(),1);
    let files=Mock::new(|_|batch(vec![file(1,"File:A.jpg"),file(2,"File:B.jpg")],Some("more")));let mut o=options(0,false);o.max_files=1;assert!(fetch(&files.client(),&o).err().unwrap().contains("more than 1 files"));assert_eq!(files.seen.lock().unwrap().len(),1,"stop downloading as soon as the file budget is exceeded");
    let invalid=Mock::new(|_|Reply{status:200,body:"not JSON".into(),retry:None});assert!(fetch(&invalid.client(),&options(0,false)).err().unwrap().contains("invalid JSON"));
    let api=Mock::new(|_|json!({"error":{"code":"badvalue","info":"invalid category"}}).into());assert!(fetch(&api.client(),&options(0,false)).err().unwrap().contains("badvalue"));
    let http=Mock::new(|_|Reply{status:403,body:"{}".into(),retry:None});assert!(fetch(&http.client(),&options(0,false)).err().unwrap().contains("HTTP 403"));
    let missing=Mock::new(|r|if r.query.get("list").is_some(){batch(vec![file(1,"File:A.jpg")],None)}else{json!({"query":{"pages":[{"pageid":1,"missing":true}]}}).into()});assert!(fetch(&missing.client(),&options(0,true)).err().unwrap().contains("disappeared"));
    let mut count=Counts{requests:MAX_REQUESTS,..Counts::default()};assert!(http.client().query(&[],&mut count).unwrap_err().contains("2000-request"));
}

#[test]
fn maxlag_retry_is_bounded_and_cancellation_during_backoff_stops_work(){
    let tries=Arc::new(Mutex::new(0));let calls=tries.clone();let mock=Mock::new(move|_|{let mut n=calls.lock().unwrap();*n+=1;if *n==1{Reply{status:503,body:json!({"error":{"code":"maxlag","info":"busy"}}).to_string(),retry:Some("0".into())}}else{batch(vec![file(1,"File:A.jpg")],None)}});
    let snapshot=fetch(&mock.client(),&options(0,false)).unwrap();assert_eq!(snapshot.counts.requests,2);
    let refused=Mock::new(|_|Reply{status:429,body:"{}".into(),retry:Some("120".into())});assert!(fetch(&refused.client(),&options(0,false)).err().unwrap().contains("120-second"));assert_eq!(refused.seen.lock().unwrap().len(),1);
    let (notified,ready)=mpsc::channel();let notified=Mutex::new(notified);let mock=Mock::new(move|_|{let _=notified.lock().unwrap().send(());Reply{status:429,body:"{}".into(),retry:Some("30".into())}});
    let client=mock.client();let owner="commons-cancel-backoff";
    let worker=thread::spawn(move||jobs::with_owner(Some(owner),||jobs::run_result("Working",||fetch(&client,&options(0,false)))));
    ready.recv_timeout(Duration::from_secs(3)).unwrap();assert!(jobs::cancel_owned(owner,0));
    assert!(worker.join().unwrap().err().unwrap().contains(jobs::CANCELLED));assert_eq!(mock.seen.lock().unwrap().len(),1);
}

#[test]
fn local_cache_is_bounded_expiring_and_preview_clipping_never_changes_the_saved_data(){
    let mut local=Cache::default();let mut first=String::new();for i in 0..5{let s=local.insert(simple_snapshot()).unwrap();if i==0{first=s.id;}}
    assert_eq!(local.snapshots.len(),4);assert!(local.get(&first).is_err());
    local.snapshots[0].created=Instant::now()-TTL-Duration::from_secs(1);let old=local.snapshots[0].id.clone();assert!(local.get(&old).is_err());
    let full="界".repeat(PREVIEW_CHARS+10);let mut s=simple_snapshot();s.prepared.table=table(&[File{id:1,title:"File:Unicode.png".into(),wikitext:Some(full.clone())}],true).unwrap();
    let view=preview_snapshot(&s,0,20).unwrap();assert_eq!(view["previewTruncated"],true);assert_eq!(view["rows"][0][1][1].as_str().unwrap().chars().count(),PREVIEW_CHARS);assert_eq!(s.prepared.table.columns[1].get(0).as_str(),Some(full.as_str()));assert!(preview_snapshot(&s,0,101).is_err());
    let mut large=simple_snapshot();large.bytes=MAX_BYTES+1;assert!(local.insert(large).is_err());
}

#[test]
fn staged_creation_saves_recon_reopens_and_preserves_current_project_on_errors_or_cancel(){
    use crate::{dispatch,Project,Session,store::Store};
    let root=std::env::temp_dir().join(format!("commons-create-{}-{}",std::process::id(),crate::recon::new_id()));let store=Store::open(&root).unwrap();let mut session=Session::with_store(store.clone());
    let snapshot=cache().lock().unwrap().insert(simple_snapshot()).unwrap();
    let created=dispatch(&mut session,"commons_create",json!({"id":snapshot.id,"name":"Commons test"})).unwrap();let id=created["data"]["id"].as_str().unwrap().to_string();
    assert_eq!(created["data"]["cursor"],0);assert_eq!(created["data"]["history"],json!([]));
    let (meta,table,history)=store.load(&id).unwrap();assert_eq!(meta.source.as_ref().unwrap().format.as_deref(),Some("commons"));assert_eq!(table.columns[0].recon(0).unwrap().matched.as_ref().unwrap().id,"M123");assert_eq!(history.cursor,0);
    session.project=Some(Project::restore(meta,table,history).unwrap());let before=store.list().len();assert!(dispatch(&mut session,"commons_create",json!({"id":"expired"})).is_err());assert_eq!(session.project.as_ref().unwrap().name,"Commons test");assert_eq!(store.list().len(),before);
    // Hold the private staging lock until the real dispatcher has registered its job, then cancel
    // exactly that owner before it can reach Store::create. No production test endpoint is exposed.
    let guard=cache().lock().unwrap();let staged_id=snapshot.id.clone();let owner="commons-cancel-create";
    let worker=thread::spawn(move||{let result=jobs::with_owner(Some(owner),||dispatch(&mut session,"commons_create",json!({"id":staged_id,"name":"must not exist"})));(session,result)});
    let until=Instant::now()+Duration::from_secs(3);loop{if jobs::list_owned(owner).as_array().unwrap().iter().any(|j|j["state"]=="running"){break;}assert!(Instant::now()<until);thread::sleep(Duration::from_millis(5));}
    assert!(jobs::cancel_owned(owner,0));drop(guard);let (session,result)=worker.join().unwrap();assert!(result.unwrap_err().contains(jobs::CANCELLED));assert_eq!(session.project.as_ref().unwrap().name,"Commons test");assert_eq!(store.list().len(),before);
}

#[test]
fn options_reject_arbitrary_endpoints_and_invalid_or_unbounded_inputs(){
    for params in [json!({}),json!({"requestId":"id","categories":[]}),json!({"requestId":"id","categories":[{"name":"https://example.com/api.php"}]}),json!({"requestId":"id","categories":[{"name":"Root","depth":11}]}),json!({"requestId":"id","categories":[{"name":"Root"}],"endpoint":"http://127.0.0.1"}),json!({"requestId":"bad\nnonce","categories":[{"name":"Root"}]}),json!({"requestId":"id","maxFiles":0,"categories":[{"name":"Root"}]})]{assert!(Options::parse(params).is_err());}
    assert_eq!(Options::parse(json!({"requestId":"valid-uuid","categories":[{"name":"category:My_test"}]})).unwrap().categories[0].name,"Category:My test");
}
