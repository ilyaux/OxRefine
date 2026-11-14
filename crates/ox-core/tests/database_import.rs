use ox_core::{database, grel::Value, jobs};
use serde_json::{json,Value as J};
use sqlx::{ConnectOptions,Connection};
use std::{path::Path,sync::Mutex,time::{Duration,Instant}};

static SERIAL:Mutex<()>=Mutex::new(());
struct Fixture{_dir:tempfile::TempDir,path:std::path::PathBuf}
impl Fixture{
    fn new()->Self{
        let dir=tempfile::tempdir().unwrap();let path=dir.path().join("synthetic.sqlite");
        write(&path,include_str!("../../../tests/database/sqlite-fixture.sql"));Self{_dir:dir,path}
    }
    fn profile(&self)->J{json!({"id":"fixture","revision":"one","name":"Synthetic SQLite","driver":"sqlite","sqlitePath":self.path,"tls":"disable"})}
    fn request(&self,sql:&str)->J{json!({"profile":self.profile(),"credentials":{"password":""},"requestId":"database-fixture","sql":sql})}
}
fn write(path:&Path,sql:&str){
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async{
        let mut c=sqlx::sqlite::SqliteConnectOptions::new().filename(path).create_if_missing(true).disable_statement_logging().connect().await.unwrap();
        sqlx::raw_sql(sql).execute(&mut c).await.unwrap();c.close().await.unwrap();
    });
}
fn cell(v:&Value)->J{match v{Value::Null=>J::Null,Value::Str(s)=>json!({"v":s}),Value::Int(n)=>json!({"v":n}),Value::Num(n)=>json!({"v":n}),other=>panic!("Unexpected database value {other:?}")}}
fn compare_reference(driver:&str,table:&ox_core::table::Table){
    let name=if driver=="sqlite"{"reference-sqlite.json"}else if driver=="postgres"{"reference-postgres.json"}else if driver=="mariadb"{"reference-mariadb.json"}else{"reference-mysql.json"};
    let file=Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/database").join(name);let reference:J=serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
    assert_eq!(json!(table.names),reference["columns"]);let rows=reference["rows"].as_array().unwrap();assert_eq!(table.rows,rows.len());
    for(r,row)in rows.iter().enumerate(){let cells=if driver=="sqlite"{row.as_array().unwrap()}else{row["cells"].as_array().unwrap()};for(c,expected)in cells.iter().enumerate(){
        let value=table.columns[c].get(r);let got=cell(value);if got==*expected{continue;}
        match table.names[c].as_str(){
            "exactv"=>{use std::str::FromStr;assert_eq!(bigdecimal::BigDecimal::from_str(value.as_str().unwrap()).unwrap(),bigdecimal::BigDecimal::from_str(expected["v"].as_str().unwrap()).unwrap());},
            "emptyv" if r<2=>{assert!(expected.is_null());assert_eq!(value.as_str(),Some(""));},
            "bytesv" if r<2=>{assert_eq!(value.as_str(),Some(if r==0{"base64:AAF//w=="}else{"base64:"}));},
            "numericv" if driver=="sqlite"&&r==0=>{assert_eq!(expected["v"],"12.5");assert_eq!(value,&Value::Num(12.5));},
            "timev"=>{assert_eq!(chrono::NaiveTime::parse_from_str(value.as_str().unwrap(),"%H:%M:%S%.f").unwrap(),chrono::NaiveTime::parse_from_str(expected["v"].as_str().unwrap(),"%H:%M:%S%.f").unwrap());},
            "stampv"=>{assert_eq!(chrono::NaiveDateTime::parse_from_str(value.as_str().unwrap(),"%Y-%m-%d %H:%M:%S%.f").unwrap(),chrono::NaiveDateTime::parse_from_str(expected["v"].as_str().unwrap(),"%Y-%m-%d %H:%M:%S%.f").unwrap());},
            "zstampv"=>{
                let got=chrono::DateTime::parse_from_rfc3339(value.as_str().unwrap()).unwrap().naive_utc();let original=expected["v"].as_str().unwrap();
                let expected=if driver=="postgres"{let normalized=if original.ends_with("+00"){format!("{original}:00")}else{original.into()};chrono::DateTime::parse_from_str(&normalized,"%Y-%m-%d %H:%M:%S%.f%:z").unwrap().naive_utc()}else{chrono::NaiveDateTime::parse_from_str(original,"%Y-%m-%d %H:%M:%S%.f").unwrap()};
                assert_eq!(got,expected);
            },
            _=>panic!("{driver} row {r} column {} expected {expected}, got {got}",table.names[c]),
        }
    }}
}
#[test]
fn sqlite_schema_typed_snapshot_and_immutable_preview_are_real_reads(){
    let _guard=SERIAL.lock().unwrap_or_else(|e|e.into_inner());let f=Fixture::new();let bytes=std::fs::read(&f.path).unwrap();
    assert_eq!(database::test(f.request("" )).unwrap()["ok"],true);
    let schema=database::schema(f.request("")).unwrap();assert_eq!(schema["tables"][0]["name"],"typed_fixture");assert_eq!(schema["tables"][0]["columns"].as_array().unwrap().len(),13);
    let view=database::prepare(f.request("SELECT * FROM typed_fixture ORDER BY id")).unwrap();let id=view["id"].as_str().unwrap();assert_eq!(view["total"],3);
    assert_eq!(view["rows"][0][1][1],i64::MAX.to_string());assert_eq!(view["rows"][1][1][1],i64::MIN.to_string());
    let p=database::prepared(id).unwrap();let t=&p.table;
    compare_reference("sqlite",t);
    assert_eq!(t.columns[1].get(0),&Value::Int(i64::MAX));assert_eq!(t.columns[1].get(1),&Value::Int(i64::MIN));
    assert_eq!(t.columns[2].get(0),&Value::Num(1.25));assert_eq!(t.columns[4].get(0).as_str(),Some("123456789012345678901234567890.12345678"));
    assert_eq!(t.columns[9].get(0).as_str(),Some("Zażółć Привет 東京"));assert_eq!(t.columns[10].get(0).as_str(),Some(""));assert!(t.columns[11].get(0).is_null());
    assert_eq!(t.columns[12].get(0).as_str(),Some("base64:AAF//w=="));assert_eq!(t.columns[12].get(1).as_str(),Some("base64:"));assert!(t.columns[12].get(2).is_null());
    assert_eq!(std::fs::read(&f.path).unwrap(),bytes,"read-only operations must leave the source unchanged");
    write(&f.path,"UPDATE typed_fixture SET textv='changed after preview' WHERE id=1");
    assert_eq!(database::prepared(id).unwrap().table.columns[9].get(0).as_str(),Some("Zażółć Привет 東京"));
    assert_eq!(database::preview(id,1,1).unwrap()["rows"].as_array().unwrap().len(),1);assert!(database::preview(id,0,101).is_err());
    assert_eq!(p.profile_id,"fixture");assert_eq!(p.profile_revision,"one");
    let failed=database::prepare(f.request("SELECT nonexistent FROM typed_fixture")).unwrap_err();assert!(!failed.contains("nonexistent"),"server SQL text must not leak in errors");
    assert!(database::prepared(id).is_ok());
}

#[test]
fn sqlite_limits_writes_and_cancellation_do_not_publish_partial_results(){
    let _guard=SERIAL.lock().unwrap_or_else(|e|e.into_inner());let f=Fixture::new();let before=std::fs::read(&f.path).unwrap();
    for sql in ["DELETE FROM typed_fixture","SELECT 1; DELETE FROM typed_fixture","WITH deleted AS (DELETE FROM typed_fixture RETURNING *) SELECT * FROM deleted","SELECT 1 INTO OUTFILE 'private'","SELECT 1--x INTO OUTFILE 'private'","SELECT 1 /*!50000 INTO OUTFILE 'private' */","SELECT 1 /* outer /* inner */ INTO OUTFILE 'private' -- */"]{
        assert!(database::prepare(f.request(sql)).is_err(),"must reject {sql}");
    }
    let mut limited=f.request("SELECT * FROM typed_fixture");limited["maxRows"]=json!(2);assert!(database::prepare(limited).unwrap_err().contains("exceeds 2 rows"));
    let mut timeout=f.request("WITH RECURSIVE n(x) AS (VALUES(0) UNION ALL SELECT x+1 FROM n WHERE x<1000000000) SELECT SUM(x) FROM n");timeout["timeoutSeconds"]=json!(1);
    let begin=Instant::now();assert!(database::prepare(timeout).unwrap_err().contains("timed out"));assert!(begin.elapsed()<Duration::from_secs(5));
    let request=f.request("WITH RECURSIVE n(x) AS (VALUES(0) UNION ALL SELECT x+1 FROM n WHERE x<1000000000) SELECT SUM(x) FROM n");let owner="database-recursive-cancel";
    let worker=std::thread::spawn(move||jobs::with_owner(Some(owner),||jobs::run_result("Working",||database::prepare(request))));
    let end=Instant::now()+Duration::from_secs(5);let id=loop{
        let list=jobs::list_owned(owner);if let Some(job)=list.as_array().unwrap().iter().find(|j|j["state"]=="running"&&j["stages"].as_array().unwrap().iter().any(|s|s["label"]=="Database request database-fixture")){break job["id"].as_u64().unwrap();}
        assert!(Instant::now()<end);std::thread::sleep(Duration::from_millis(5));
    };
    // Let the recursive statement reach sqlite3_step before cancelling its exact job.
    std::thread::sleep(Duration::from_millis(100));let begin=Instant::now();assert!(jobs::cancel_owned(owner,id));let error=worker.join().unwrap().unwrap_err();assert!(jobs::is_cancelled(&error));assert!(begin.elapsed()<Duration::from_secs(3));
    assert_eq!(database::test(f.request("")).unwrap()["ok"],true,"SQLite worker must really stop, not keep the file occupied");
    assert_eq!(std::fs::read(&f.path).unwrap(),before);
}

#[test]
fn real_configured_network_databases_preserve_exact_numbers(){
    let _guard=SERIAL.lock().unwrap_or_else(|e|e.into_inner());let Some(path)=std::env::var_os("OXREFINE_DATABASE_TEST_CONFIG")else{eprintln!("Network database test not configured; no server validation claimed");return;};
    let config:J=serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();let connections=config["connections"].as_array().unwrap();let mut checked=0;
    for c in connections{
        let password=c.get("password").cloned().unwrap_or(json!(""));let mut profile=json!({"name":"Synthetic server fixture","id":"server-fixture","revision":"one"});
        for key in ["driver","host","port","database","username","tls"]{if let Some(value)=c.get(key){profile[key]=value.clone();}}
        let request=json!({"profile":profile,"credentials":{"password":password},"requestId":"database-server-fixture","sql":"SELECT * FROM typed_fixture ORDER BY id"});
        assert_eq!(database::test(request.clone()).unwrap_or_else(|e|panic!("{} test: {e}",c["driver"]))["ok"],true);assert!(!database::schema(request.clone()).unwrap_or_else(|e|panic!("{} schema: {e}",c["driver"]))["tables"].as_array().unwrap().is_empty());
        let view=database::prepare(request.clone()).unwrap_or_else(|e|panic!("{} query: {e}",c["driver"]));let p=database::prepared(view["id"].as_str().unwrap()).unwrap();let t=&p.table;assert_eq!(t.rows,3);
        compare_reference(c["driver"].as_str().unwrap(),t);
        let exact=t.col("exactv").unwrap();assert_eq!(t.columns[exact].get(0).as_str(),Some("123456789012345678901234567890.12345678"));
        let big=t.col("bigv").unwrap();assert_eq!(t.columns[big].get(0),&Value::Int(i64::MAX));assert_eq!(t.columns[big].get(1),&Value::Int(i64::MIN));
        let mut duplicate=request.clone();duplicate["sql"]=json!("SELECT 1 AS label, 2 AS label");let view=database::prepare(duplicate).unwrap();assert_eq!(view["columns"],json!(["label","label2"]));
        let mut empty=request.clone();empty["sql"]=json!("SELECT * FROM typed_fixture WHERE 1=0");let view=database::prepare(empty).unwrap();assert_eq!(view["total"],0);assert_eq!(view["columns"].as_array().unwrap().len(),14);
        let digits="12345678901234567890123456789.123456789";
        let json_queries=if c["driver"]=="postgres"{vec![format!("SELECT '{{\"nested\":[{{\"n\":{digits}}}]}}'::json AS value"),format!("SELECT '{{\"nested\":[{{\"n\":{digits}}}]}}'::jsonb AS value")]}else{vec![format!("SELECT JSON_OBJECT('n',CAST('{digits}' AS DECIMAL(38,9))) AS value")]};
        for query in json_queries{let mut exact=request.clone();exact["sql"]=json!(query);let view=database::prepare(exact).unwrap();let table=database::prepared(view["id"].as_str().unwrap()).unwrap().table;assert!(table.columns[0].get(0).as_str().unwrap().contains(digits),"{} nested JSON number must remain exact",c["driver"]);}
        if c["driver"]!="postgres"{
            let mut unsigned=request.clone();unsigned["sql"]=json!("SELECT uintv FROM unsigned_fixture ORDER BY id");let view=database::prepare(unsigned).unwrap();let table=database::prepared(view["id"].as_str().unwrap()).unwrap().table;assert_eq!(table.columns[0].get(0).as_str(),Some("18446744073709551615"));
            let mut booleans=request.clone();booleans["sql"]=json!("SELECT boolv FROM boolean_storage_fixture ORDER BY id");let view=database::prepare(booleans).unwrap();let table=database::prepared(view["id"].as_str().unwrap()).unwrap().table;assert_eq!((0..table.rows).map(|r|table.columns[0].get(r).as_str().unwrap()).collect::<Vec<_>>(),vec!["2","-7","0"]);
            let mut tiny_unsigned=request.clone();tiny_unsigned["sql"]=json!("SELECT boolv FROM JSON_TABLE('[127,128,255]', '$[*]' COLUMNS (boolv TINYINT(1) UNSIGNED PATH '$')) AS jt ORDER BY boolv");let view=database::prepare(tiny_unsigned).unwrap();assert_eq!(view["columnTypes"][0],"BOOLEAN");let table=database::prepared(view["id"].as_str().unwrap()).unwrap().table;assert_eq!((0..table.rows).map(|r|table.columns[0].get(r).as_str().unwrap()).collect::<Vec<_>>(),vec!["127","128","255"]);
            // Native HEX/CAST probes confirm MariaDB coerces numeric JSON 0/1 to ASCII for BIT.
            let document=if c["driver"]=="mariadb"{"JSON_ARRAY(CHAR(0 USING utf8mb4),CHAR(1 USING utf8mb4),18446744073709551615,NULL)"}else{"'[0,1,18446744073709551615,null]'"};
            let mut bits=request.clone();bits["sql"]=json!(format!("SELECT bitv FROM JSON_TABLE({document}, '$[*]' COLUMNS (bitv BIT(64) PATH '$')) AS jt"));let view=database::prepare(bits).unwrap();assert_eq!(view["columnTypes"][0],"BIT");let table=database::prepared(view["id"].as_str().unwrap()).unwrap().table;assert_eq!((0..3).map(|r|table.columns[0].get(r).as_str().unwrap()).collect::<Vec<_>>(),vec!["base64:AAAAAAAAAAA=","base64:AAAAAAAAAAE=","base64://////////8="]);assert!(table.columns[0].get(3).is_null());
            let mut negative_time=request;negative_time["sql"]=json!("SELECT CAST('-125:12:34.500000' AS TIME(6)) AS duration");let view=database::prepare(negative_time).unwrap();let table=database::prepared(view["id"].as_str().unwrap()).unwrap().table;assert_eq!(table.columns[0].get(0).as_str(),Some("-125:12:34.500000"));
        }
        checked+=1;
    }assert!(checked>0);
}

#[test]
fn live_network_timeouts_and_exact_job_cancellation_stop_server_queries(){
    let _guard=SERIAL.lock().unwrap_or_else(|e|e.into_inner());let Some(path)=std::env::var_os("OXREFINE_DATABASE_TEST_CONFIG")else{eprintln!("Network cancellation test not configured; no server validation claimed");return;};
    let config:J=serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    for c in config["connections"].as_array().unwrap(){
        let driver=c["driver"].as_str().unwrap();let mut profile=json!({"name":"Synthetic cancellation"});for key in["driver","host","port","database","username","tls"]{if let Some(value)=c.get(key){profile[key]=value.clone();}}
        let long=if driver=="postgres"{"SELECT pg_sleep(20)"}else{"SELECT SLEEP(20)"};let request=json!({"profile":profile,"credentials":{"password":c["password"]},"requestId":"live-server-cancel","sql":long,"timeoutSeconds":1});
        let begin=Instant::now();let error=database::prepare(request.clone()).unwrap_err();assert!(error.contains("timed out"),"{driver}: {error}");assert!(begin.elapsed()<Duration::from_secs(6));
        let mut cancel=request.clone();cancel["timeoutSeconds"]=json!(30);let owner=format!("database-live-cancel-{driver}");let task_owner=owner.clone();
        let worker=std::thread::spawn(move||jobs::with_owner(Some(&task_owner),||jobs::run_result("Working",||database::prepare(cancel))));
        let end=Instant::now()+Duration::from_secs(5);let id=loop{let list=jobs::list_owned(&owner);if let Some(j)=list.as_array().unwrap().iter().find(|j|j["state"]=="running"&&j["stages"].as_array().unwrap().iter().any(|s|s["label"]=="Database request live-server-cancel")){break j["id"].as_u64().unwrap();}assert!(Instant::now()<end);std::thread::sleep(Duration::from_millis(5));};
        std::thread::sleep(Duration::from_millis(150));let begin=Instant::now();assert!(jobs::cancel_owned(&owner,id));let error=worker.join().unwrap().unwrap_err();assert!(jobs::is_cancelled(&error),"{driver}: {error}");assert!(begin.elapsed()<Duration::from_secs(6));
        let mut probe=request;probe["timeoutSeconds"]=json!(5);probe["sql"]=json!(if driver=="postgres"{"SELECT COUNT(*) AS active FROM pg_stat_activity WHERE usename = current_user AND state = 'active' AND query = 'SELECT pg_sleep(20)'"}else{"SELECT COUNT(*) AS active FROM information_schema.PROCESSLIST WHERE INFO = 'SELECT SLEEP(20)'"});
        let view=database::prepare(probe).unwrap_or_else(|e|panic!("{driver} activity probe: {e}"));let table=database::prepared(view["id"].as_str().unwrap()).unwrap().table;assert_eq!(table.columns[0].get(0),&Value::Int(0),"{driver} cancelled statement must not keep running on the server");
    }
}

#[test]
fn staged_database_create_saves_without_a_second_query_and_supports_spilled_codes(){
    use ox_core::{dispatch,Session,store::Store,spill::CodeBudget,storage};
    let _guard=SERIAL.lock().unwrap_or_else(|e|e.into_inner());let f=Fixture::new();let store_dir=tempfile::tempdir().unwrap();
    let spill=tempfile::tempdir().unwrap();let budget=CodeBudget::new(0,spill.path().into());let store=Store::open_with_budget(store_dir.path(),budget.clone()).unwrap();let mut session=Session::with_store(store.clone());
    let view=storage::with_budget(budget.clone(),||database::prepare(f.request("SELECT id,intv,textv,emptyv,nullable,bytesv FROM typed_fixture ORDER BY id"))).unwrap();let id=view["id"].as_str().unwrap();
    let p=database::prepared(id).unwrap();assert!(p.table.columns.iter().all(|c|c.codes.is_mapped()));
    let hosted=dispatch(&mut session,"database_prepare",json!({"params":f.request("SELECT id,intv,textv,emptyv,nullable,bytesv FROM typed_fixture ORDER BY id")})).unwrap();let host_id=hosted["data"]["id"].as_str().unwrap();
    assert!(database::prepared(host_id).unwrap().table.columns.iter().all(|c|c.codes.is_mapped()),"ordinary host dispatch must inherit the Store budget without a manual scope");
    // Removing the source makes any accidental repeat query fail, but creation uses the snapshot.
    std::fs::remove_file(&f.path).unwrap();let result=dispatch(&mut session,"database_create",json!({"id":host_id,"name":"Saved database snapshot"})).unwrap();
    let saved=result["data"]["id"].as_str().unwrap();let(meta,table,history)=store.load(saved).unwrap();assert_eq!(meta.name,"Saved database snapshot");assert_eq!(table.rows,3);assert_eq!(table.columns[1].get(0),&Value::Int(i64::MAX));assert_eq!(table.columns[3].get(0).as_str(),Some(""));assert!(table.columns[4].get(0).is_null());assert_eq!(history.cursor,0);
    let source=serde_json::to_string(&meta.source).unwrap();assert!(!source.contains("SELECT"));assert!(!source.contains("password"));assert!(!source.contains("synthetic.sqlite"));
    let before=store.list().len();assert!(dispatch(&mut session,"database_create",json!({"id":"expired"})).is_err());assert_eq!(store.list().len(),before);assert_eq!(session.project.as_ref().unwrap().name,"Saved database snapshot");
}
