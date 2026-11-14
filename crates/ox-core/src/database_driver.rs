use super::*;
use futures_util::TryStreamExt;
use sqlx::{Column as _, ConnectOptions, Connection, Executor, Row, TypeInfo, ValueRef};
use std::sync::atomic::{AtomicU64,AtomicUsize};

pub(super) enum Mode<'a>{Test,Schema,Query(&'a str)}
pub(super) enum Output{Test,Schema(J),Rows(ResultRows)}
struct Control {stop:Arc<AtomicBool>,deadline:Instant,connection_id:Arc<AtomicU64>}
static ACTIVE:AtomicUsize=AtomicUsize::new(0);
struct Permit;
impl Drop for Permit{fn drop(&mut self){ACTIVE.fetch_sub(1,Ordering::Relaxed);}}

fn sql_error(error:sqlx::Error)->String {
    // Database error messages can contain values, SQL and connection secrets. Only expose codes.
    match error {
        // A server statement deadline can fire before the client's clock; SQLSTATE 70100 alone
        // also means an ordinary interruption, so match the precise vendor timeout numbers.
        sqlx::Error::Database(e) if e.try_downcast_ref::<sqlx::mysql::MySqlDatabaseError>().is_some_and(|e|matches!(e.number(),1969|3024))=>"Database request timed out; no snapshot was kept".into(),
        sqlx::Error::Database(e)=>{let code=e.code().map(|s|s.chars().filter(|c|c.is_ascii_alphanumeric()).take(16).collect::<String>()).unwrap_or_default();format!("Database query failed (SQLSTATE/code {code}). Check the query, permissions and schema.")},
        sqlx::Error::Tls(_)=>"Database TLS verification failed. Check the server certificate and TLS setting.".into(),
        sqlx::Error::Io(_)=>"Database network connection failed. Check the host, port and server availability.".into(),
        sqlx::Error::ColumnDecode{..}|sqlx::Error::Decode(_)=>"A database cell has an unsupported or unreadable type; cast it to text in the query.".into(),
        _=>"Database operation failed. Check the connection settings, query and supported data types.".into(),
    }
}
fn pg_options(profile:&Profile,password:&str)->sqlx::postgres::PgConnectOptions {
    sqlx::postgres::PgConnectOptions::new_without_pgpass().host(&profile.host).port(profile.port).database(&profile.database).username(&profile.username).password(password)
        .ssl_mode(if profile.tls=="disable"{sqlx::postgres::PgSslMode::Disable}else{sqlx::postgres::PgSslMode::VerifyFull}).application_name("OxRefine database import").statement_cache_capacity(0).disable_statement_logging()
}
fn mysql_options(profile:&Profile,password:&str)->sqlx::mysql::MySqlConnectOptions {
    sqlx::mysql::MySqlConnectOptions::new().host(&profile.host).port(profile.port).database(&profile.database).username(&profile.username).password(password)
        .ssl_mode(if profile.tls=="disable"{sqlx::mysql::MySqlSslMode::Disabled}else{sqlx::mysql::MySqlSslMode::VerifyIdentity}).charset("utf8mb4").timezone(Some("+00:00".into())).statement_cache_capacity(0).disable_statement_logging()
}
pub(super) fn run(request:&Request,profile:&Profile,mode:Mode<'_>)->Result<Output>{
    jobs::progress(&format!("Database request {}",request.request_id),0);jobs::tick(0)?;
    ACTIVE.fetch_update(Ordering::Relaxed,Ordering::Relaxed,|n|(n<4).then_some(n+1)).map_err(|_|"Four database operations are already running; wait for one to finish")?;let _permit=Permit;
    let runtime=tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|_|"Could not start the database I/O runtime")?;
    let control=Control{stop:Arc::new(AtomicBool::new(false)),deadline:Instant::now()+Duration::from_secs(request.timeout_seconds),connection_id:Arc::new(AtomicU64::new(0))};
    let result=runtime.block_on(async {
        let operation=async {match profile.driver.as_str(){"sqlite"=>sqlite(request,profile,&control,mode).await,"postgres"=>postgres(request,profile,&control,mode).await,_=>mysql(request,profile,&control,mode).await}};
        let mut operation=Box::pin(operation);
        loop {
            match tokio::time::timeout(Duration::from_millis(50),operation.as_mut()).await {Ok(result)=>{jobs::tick(0)?;if Instant::now()>=control.deadline{return Err("Database request timed out; no snapshot was kept".into());}return result;},Err(_)=>{}}
            let reason=if let Err(reason)=jobs::tick(0){Some(reason)}else if Instant::now()>=control.deadline{Some("Database request timed out; no snapshot was kept".into())}else{None};
            if let Some(reason)=reason {
                control.stop.store(true,Ordering::Relaxed);
                let id=control.connection_id.load(Ordering::Relaxed);
                if id>0{let _=tokio::time::timeout(Duration::from_secs(3),cancel_server(profile,&request.credentials.password,id)).await;}
                // SQLite's progress callback interrupts its worker; network drivers get an own-id
                // cancel request. Allow rollback to finish, then drop this one-use connection.
                let _=tokio::time::timeout(Duration::from_secs(1),operation.as_mut()).await;
                return Err(reason);
            }
        }
    });
    // A cancelled OS hostname lookup may outlive its async future; do not wait indefinitely for it.
    runtime.shutdown_timeout(Duration::from_millis(100));result
}
async fn cancel_server(profile:&Profile,password:&str,id:u64)->Result<()> {
    if profile.driver=="postgres"{
        let mut connection=pg_options(profile,password).connect().await.map_err(sql_error)?;
        sqlx::query("SELECT pg_cancel_backend($1)").bind(id as i32).execute(&mut connection).await.map_err(sql_error)?;
        connection.close().await.map_err(sql_error)
    }else{
        let mut connection=mysql_options(profile,password).connect().await.map_err(sql_error)?;
        connection.execute(format!("KILL QUERY {id}").as_str()).await.map_err(sql_error)?;
        connection.close().await.map_err(sql_error)
    }
}
fn check(control:&Control)->Result<()> {
    if control.stop.load(Ordering::Relaxed){return Err(jobs::CANCELLED.into());}
    jobs::tick(0)?;if Instant::now()>=control.deadline{return Err("Database request timed out; no snapshot was kept".into());}Ok(())
}

async fn sqlite(request:&Request,profile:&Profile,control:&Control,mode:Mode<'_>)->Result<Output>{
    let path=std::fs::canonicalize(Path::new(&profile.sqlite_path)).map_err(|_|"SQLite database file does not exist or cannot be read")?;
    if !path.is_file(){return Err("SQLite import needs an existing database file".into());}
    let options=sqlx::sqlite::SqliteConnectOptions::new().filename(path).read_only(true).create_if_missing(false).pragma("query_only","ON").busy_timeout(Duration::from_secs(3)).disable_statement_logging();
    let mut connection=options.connect().await.map_err(sql_error)?;
    {let stop=control.stop.clone();let deadline=control.deadline;connection.lock_handle().await.map_err(sql_error)?.set_progress_handler(1000,move||!stop.load(Ordering::Relaxed)&&Instant::now()<deadline);}
    connection.execute("BEGIN").await.map_err(sql_error)?;
    let result:Result<Output>=async {match mode {
        Mode::Test=>{connection.ping().await.map_err(sql_error)?;Ok(Output::Test)},
        Mode::Schema=>{
            let tables=sqlx::query("SELECT name FROM sqlite_schema WHERE type IN ('table','view') AND name NOT LIKE 'sqlite_%' ORDER BY name LIMIT 501").fetch_all(&mut connection).await.map_err(sql_error)?;
            if tables.len()>500{return Err("Database schema exceeds 500 tables; choose a smaller database".into());}
            let mut out=vec![];let mut total=0;
            for table in tables {check(control)?;let name:String=table.try_get(0).map_err(sql_error)?;
                let columns=sqlx::query("SELECT name,type,\"notnull\",pk FROM pragma_table_info(?)").bind(&name).fetch_all(&mut connection).await.map_err(sql_error)?;
                total+=columns.len();if total>10000{return Err("Database schema exceeds 10000 columns".into());}
                out.push(json!({"schema":"main","name":name,"columns":columns.iter().map(|c|Ok(json!({"name":c.try_get::<String,_>(0)?,"type":c.try_get::<String,_>(1)?,"nullable":c.try_get::<i64,_>(2)?==0&&c.try_get::<i64,_>(3)?==0}))).collect::<std::result::Result<Vec<_>,sqlx::Error>>().map_err(sql_error)?}));
            }Ok(Output::Schema(json!({"tables":out})))
        },
        Mode::Query(sql)=>{
            let description=connection.describe(sql).await.map_err(sql_error)?;
            let mut rows=Rows::new(description.columns().iter().map(|c|(c.name().into(),c.type_info().name().into())).collect(),request.max_rows)?;
            let mut stream=sqlx::query(sql).fetch(&mut connection);
            while let Some(row)=stream.try_next().await.map_err(sql_error)?{check(control)?;let values=(0..row.len()).map(|i|sqlite_value(&row,i)).collect::<Result<Vec<_>>>()?;rows.push(values)?;}
            Ok(Output::Rows(rows.finish()?))
        },
    }}.await;
    let rollback=connection.execute("ROLLBACK").await;let closed=connection.close().await;
    let output=result?;rollback.map_err(sql_error)?;closed.map_err(sql_error)?;Ok(output)
}
fn sqlite_value(row:&sqlx::sqlite::SqliteRow,i:usize)->Result<Value>{
    let raw=row.try_get_raw(i).map_err(sql_error)?;if raw.is_null(){return Ok(Value::Null);}
    let value=match raw.type_info().name(){
        "INTEGER"=>Value::Int(row.try_get(i).map_err(sql_error)?),
        "REAL"=>finite(row.try_get(i).map_err(sql_error)?),
        "BLOB"=>binary(row.try_get(i).map_err(sql_error)?),
        _=>Value::Str(row.try_get(i).map_err(sql_error)?),
    };Ok(if matches!(row.columns()[i].type_info().name(),"BOOLEAN"|"BOOL"){match value{Value::Int(n)=>Value::Str(n.to_string()),other=>other}}else{value})
}

async fn postgres(request:&Request,profile:&Profile,control:&Control,mode:Mode<'_>)->Result<Output>{
    let mut connection=pg_options(profile,&request.credentials.password).connect().await.map_err(sql_error)?;
    // A transaction-pooling proxy must pin this backend before its cancellation id is captured.
    connection.execute("BEGIN READ ONLY").await.map_err(sql_error)?;
    let id:i32=sqlx::query_scalar("SELECT pg_backend_pid()").fetch_one(&mut connection).await.map_err(sql_error)?;control.connection_id.store(id as u64,Ordering::Relaxed);
    connection.execute(format!("SET LOCAL statement_timeout = {}",request.timeout_seconds*1000).as_str()).await.map_err(sql_error)?;
    connection.execute("SET LOCAL lock_timeout = 3000").await.map_err(sql_error)?;
    let result:Result<Output>=async {match mode{
        Mode::Test=>{connection.ping().await.map_err(sql_error)?;Ok(Output::Test)},
        Mode::Schema=>{
            let records=sqlx::query("SELECT table_schema,table_name,column_name,data_type,is_nullable FROM information_schema.columns WHERE table_schema NOT IN ('pg_catalog','information_schema') ORDER BY table_schema,table_name,ordinal_position LIMIT 10001").fetch_all(&mut connection).await.map_err(sql_error)?;
            let rows=records.into_iter().map(|r|Ok((r.try_get::<String,_>(0)?,r.try_get::<String,_>(1)?,r.try_get::<String,_>(2)?,r.try_get::<String,_>(3)?,r.try_get::<String,_>(4)?=="YES"))).collect::<std::result::Result<Vec<_>,sqlx::Error>>().map_err(sql_error)?;
            Ok(Output::Schema(schema_rows(rows)?))
        },
        Mode::Query(sql)=>{
            let description=connection.describe(sql).await.map_err(sql_error)?;
            let mut rows=Rows::new(description.columns().iter().map(|c|(c.name().into(),c.type_info().name().into())).collect(),request.max_rows)?;
            let mut stream=sqlx::query(sql).fetch(&mut connection);
            while let Some(row)=stream.try_next().await.map_err(sql_error)?{check(control)?;rows.push((0..row.len()).map(|i|pg_value(&row,i).map_err(|e|format!("PostgreSQL column {} ({}): {e}",i+1,row.columns()[i].type_info().name()))).collect::<Result<Vec<_>>>()?)?;}
            Ok(Output::Rows(rows.finish()?))
        },
    }}.await;
    let rollback=connection.execute("ROLLBACK").await;control.connection_id.store(0,Ordering::Relaxed);let closed=connection.close().await;
    let output=result?;rollback.map_err(sql_error)?;closed.map_err(sql_error)?;Ok(output)
}
fn pg_value(row:&sqlx::postgres::PgRow,i:usize)->Result<Value>{
    let raw=row.try_get_raw(i).map_err(sql_error)?;if raw.is_null(){return Ok(Value::Null);}
    Ok(match raw.type_info().name(){
        "INT2"=>Value::Int(row.try_get::<i16,_>(i).map_err(sql_error)?as i64),"INT4"=>Value::Int(row.try_get::<i32,_>(i).map_err(sql_error)?as i64),"INT8"=>Value::Int(row.try_get(i).map_err(sql_error)?),
        "FLOAT4"=>finite(row.try_get::<f32,_>(i).map_err(sql_error)?as f64),"FLOAT8"=>finite(row.try_get(i).map_err(sql_error)?),
        "NUMERIC"=>Value::Str(row.try_get::<bigdecimal::BigDecimal,_>(i).map_err(sql_error)?.to_plain_string()),
        "BOOL"=>Value::Str(if row.try_get::<bool,_>(i).map_err(sql_error)?{"t"}else{"f"}.into()),"BYTEA"=>binary(row.try_get(i).map_err(sql_error)?),
        "DATE"=>Value::Str(row.try_get::<chrono::NaiveDate,_>(i).map_err(sql_error)?.to_string()),"TIME"=>Value::Str(row.try_get::<chrono::NaiveTime,_>(i).map_err(sql_error)?.to_string()),
        "TIMESTAMP"=>Value::Str(row.try_get::<chrono::NaiveDateTime,_>(i).map_err(sql_error)?.to_string()),"TIMESTAMPTZ"=>Value::Str(row.try_get::<chrono::DateTime<chrono::Utc>,_>(i).map_err(sql_error)?.to_rfc3339()),
        "JSON"|"JSONB"=>Value::Str(row.try_get::<sqlx::types::Json<Box<serde_json::value::RawValue>>,_>(i).map_err(sql_error)?.0.get().to_owned()),
        "TEXT"|"VARCHAR"|"BPCHAR"|"NAME"|"UNKNOWN"=>Value::Str(row.try_get(i).map_err(sql_error)?),
        _=>return Err("Unsupported PostgreSQL result type; cast it to text in the query".into()),
    })
}

async fn mysql(request:&Request,profile:&Profile,control:&Control,mode:Mode<'_>)->Result<Output>{
    let mut connection=mysql_options(profile,&request.credentials.password).connect().await.map_err(sql_error)?;
    connection.execute("START TRANSACTION READ ONLY").await.map_err(sql_error)?;
    let id:u64=sqlx::query_scalar("SELECT CONNECTION_ID()").fetch_one(&mut connection).await.map_err(sql_error)?;control.connection_id.store(id,Ordering::Relaxed);
    if profile.driver=="mariadb"{connection.execute(format!("SET SESSION max_statement_time = {}",request.timeout_seconds).as_str()).await.map_err(sql_error)?;}
    else{connection.execute(format!("SET SESSION max_execution_time = {}",request.timeout_seconds*1000).as_str()).await.map_err(sql_error)?;}
    connection.execute("SET SESSION innodb_lock_wait_timeout = 3").await.map_err(sql_error)?;
    let result:Result<Output>=async {match mode{
        Mode::Test=>{connection.ping().await.map_err(sql_error)?;Ok(Output::Test)},
        Mode::Schema=>{
            let records=sqlx::query("SELECT TABLE_SCHEMA,TABLE_NAME,COLUMN_NAME,COLUMN_TYPE,IS_NULLABLE FROM information_schema.COLUMNS WHERE TABLE_SCHEMA = DATABASE() ORDER BY TABLE_NAME,ORDINAL_POSITION LIMIT 10001").fetch_all(&mut connection).await.map_err(sql_error)?;
            // MySQL 8 marks some information_schema text fields as binary metadata; these five
            // fixed expressions are still UTF-8 text, not arbitrary application BLOB columns.
            let rows=records.into_iter().map(|r|Ok((r.try_get_unchecked::<String,_>(0)?,r.try_get_unchecked::<String,_>(1)?,r.try_get_unchecked::<String,_>(2)?,r.try_get_unchecked::<String,_>(3)?,r.try_get_unchecked::<String,_>(4)?=="YES"))).collect::<std::result::Result<Vec<_>,sqlx::Error>>().map_err(sql_error)?;
            Ok(Output::Schema(schema_rows(rows)?))
        },
        Mode::Query(sql)=>{
            let description=connection.describe(sql).await.map_err(sql_error)?;
            let mut rows=Rows::new(description.columns().iter().map(|c|(c.name().into(),c.type_info().name().into())).collect(),request.max_rows)?;
            let mut stream=sqlx::query(sql).fetch(&mut connection);
            while let Some(row)=stream.try_next().await.map_err(sql_error)?{check(control)?;rows.push((0..row.len()).map(|i|mysql_value(&row,i).map_err(|e|format!("MySQL/MariaDB column {} ({}): {e}",i+1,row.columns()[i].type_info().name()))).collect::<Result<Vec<_>>>()?)?;}
            Ok(Output::Rows(rows.finish()?))
        },
    }}.await;
    let rollback=connection.execute("ROLLBACK").await;control.connection_id.store(0,Ordering::Relaxed);let closed=connection.close().await;
    let output=result?;rollback.map_err(sql_error)?;closed.map_err(sql_error)?;Ok(output)
}
fn mysql_value(row:&sqlx::mysql::MySqlRow,i:usize)->Result<Value>{
    let raw=row.try_get_raw(i).map_err(sql_error)?;if raw.is_null(){return Ok(Value::Null);}let name=raw.type_info().name().to_string();
    Ok(match name.as_str(){
        "TINYINT"|"SMALLINT"|"MEDIUMINT"|"INT"|"BIGINT"=>Value::Int(row.try_get(i).map_err(sql_error)?),
        "TINYINT UNSIGNED"|"SMALLINT UNSIGNED"|"MEDIUMINT UNSIGNED"|"INT UNSIGNED"|"BIGINT UNSIGNED"=>{let n:u64=row.try_get(i).map_err(sql_error)?;i64::try_from(n).map(Value::Int).unwrap_or_else(|_|Value::Str(n.to_string()))},
        "FLOAT"=>finite(row.try_get::<f32,_>(i).map_err(sql_error)?as f64),"DOUBLE"=>finite(row.try_get(i).map_err(sql_error)?),
        "DECIMAL"=>Value::Str(row.try_get::<bigdecimal::BigDecimal,_>(i).map_err(sql_error)?.to_plain_string()),
        // TINYINT(1) is only a display width: values such as 2 or -7 must not become true/false.
        "BOOLEAN"=>Value::Str(if <u64 as sqlx::Type<sqlx::MySql>>::compatible(row.columns()[i].type_info()){row.try_get::<u64,_>(i).map_err(sql_error)?.to_string()}else{row.try_get::<i64,_>(i).map_err(sql_error)?.to_string()}),
        "DATE"=>Value::Str(row.try_get::<chrono::NaiveDate,_>(i).map_err(sql_error)?.to_string()),"TIME"=>{let t=row.try_get::<sqlx::mysql::types::MySqlTime,_>(i).map_err(sql_error)?;Value::Str(format!("{}{:02}:{:02}:{:02}.{:06}",if t.sign().is_negative(){"-"}else{""},t.hours(),t.minutes(),t.seconds(),t.microseconds()))},
        "DATETIME"=>Value::Str(row.try_get::<chrono::NaiveDateTime,_>(i).map_err(sql_error)?.format("%Y-%m-%d %H:%M:%S%.6f").to_string()),
        "TIMESTAMP"=>Value::Str(row.try_get::<chrono::DateTime<chrono::Utc>,_>(i).map_err(sql_error)?.to_rfc3339()),
        // SQLx's checked Vec<u8> compatibility excludes BIT, whose wire payload is still bytes.
        "BIT"=>binary(row.try_get_unchecked::<Vec<u8>,_>(i).map_err(sql_error)?),
        "BINARY"|"VARBINARY"|"BLOB"|"TINYBLOB"|"MEDIUMBLOB"|"LONGBLOB"=>binary(row.try_get(i).map_err(sql_error)?),
        "JSON"=>Value::Str(row.try_get::<sqlx::types::Json<Box<serde_json::value::RawValue>>,_>(i).map_err(sql_error)?.0.get().to_owned()),
        _=>Value::Str(row.try_get(i).map_err(sql_error)?),
    })
}
fn schema_rows(rows:Vec<(String,String,String,String,bool)>)->Result<J>{
    if rows.len()>10000{return Err("Database schema exceeds 10000 columns".into());}let mut tables:Vec<J>=vec![];
    for(schema,table,name,kind,nullable)in rows{
        let same=tables.last().is_some_and(|t|t["schema"]==schema&&t["name"]==table);
        if !same{if tables.len()>=500{return Err("Database schema exceeds 500 tables".into());}tables.push(json!({"schema":schema,"name":table,"columns":[]}));}
        tables.last_mut().unwrap()["columns"].as_array_mut().unwrap().push(json!({"name":name,"type":kind,"nullable":nullable}));
    }Ok(json!({"tables":tables}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_server_timeout_is_classified_without_waiting_for_a_client_deadline(){
        let Some(path)=std::env::var_os("OXREFINE_DATABASE_TEST_CONFIG")else{eprintln!("Live server timeout classification not configured");return;};
        let config:J=serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let runtime=tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        runtime.block_on(async {
            for entry in config["connections"].as_array().unwrap().iter().filter(|c|c["driver"]=="mariadb"||c["driver"]=="mysql"){
                let mut p=json!({"name":"Server timeout regression"});for key in["driver","host","port","database","username","tls"]{if let Some(v)=entry.get(key){p[key]=v.clone();}}
                let profile=validate_profile(p).unwrap();let password=entry["password"].as_str().unwrap();let mut connection=mysql_options(&profile,password).connect().await.unwrap();
                let setting=if profile.driver=="mariadb"{"SET SESSION max_statement_time = 0.01"}else{"SET SESSION max_execution_time = 10"};connection.execute(setting).await.unwrap();connection.execute("START TRANSACTION READ ONLY").await.unwrap();
                let query="WITH RECURSIVE seq(n) AS (SELECT 1 UNION ALL SELECT n+1 FROM seq WHERE n<1000) SELECT SUM(a.n*b.n*c.n) FROM seq a CROSS JOIN seq b CROSS JOIN seq c";
                let error=tokio::time::timeout(Duration::from_secs(3),sqlx::query(query).fetch_one(&mut connection)).await.unwrap().unwrap_err();
                let database=error.as_database_error().unwrap().try_downcast_ref::<sqlx::mysql::MySqlDatabaseError>().unwrap();
                eprintln!("{} server timeout: vendor={} SQLSTATE={}",profile.driver,database.number(),database.code().unwrap_or(""));
                let expected=if profile.driver=="mariadb"{1969}else{3024};assert_eq!(database.number(),expected);
                let safe=sql_error(error);connection.execute("ROLLBACK").await.unwrap();connection.close().await.unwrap();
                assert!(safe.contains("timed out"),"{}: {safe}",profile.driver);

                let mut connection=mysql_options(&profile,password).connect().await.unwrap();let disable=if profile.driver=="mariadb"{"SET SESSION max_statement_time = 0"}else{"SET SESSION max_execution_time = 0"};connection.execute(disable).await.unwrap();connection.execute("START TRANSACTION READ ONLY").await.unwrap();
                let id:u64=sqlx::query_scalar("SELECT CONNECTION_ID()").fetch_one(&mut connection).await.unwrap();
                let interrupted=sqlx::query(query).fetch_one(&mut connection);
                let cancel=async{tokio::time::sleep(Duration::from_millis(50)).await;cancel_server(&profile,password,id).await.unwrap();};
                let(result,())=tokio::time::timeout(Duration::from_secs(3),futures_util::future::join(interrupted,cancel)).await.unwrap();let error=result.unwrap_err();
                assert_eq!(error.as_database_error().unwrap().try_downcast_ref::<sqlx::mysql::MySqlDatabaseError>().unwrap().number(),1317);
                assert!(!sql_error(error).contains("timed out"),"an explicit interruption is not a statement timeout");connection.execute("ROLLBACK").await.unwrap();connection.close().await.unwrap();
            }
        });
    }
}
