use base64::Engine as _;
use calamine::{Data,Reader};
use ox_core::{dispatch,export::{export,export_file},grel::dates,recon::{candidate_from_json,Judgment,Recon,ReconCol},store::Store,table::{Column,Table},Project,Session};
use serde_json::{json,Value as J};
use std::{io::Cursor,path::PathBuf,sync::Arc};
fn dir() -> PathBuf {let p=std::env::temp_dir().join(format!("ox-custom-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));std::fs::create_dir(&p).unwrap();p}
fn fixture()->J {serde_json::from_str(include_str!("../../../tests/export/custom-fixtures.json")).unwrap()}
fn table(f:&J)->Table {
    let path=dir().join("data.csv");std::fs::write(&path,f["corpus"].as_str().unwrap()).unwrap();let mut p=Project::open(&path).unwrap();
    for op in f["typed"].as_array().unwrap(){p.apply(serde_json::from_value(op.clone()).unwrap()).unwrap()}
    let mut t=p.table().clone();
    for ci in 0..t.columns.len(){
        let cells=f["table"]["rows"].as_array().unwrap().iter().map(|row|{
            let j=&row["cells"][ci]["r"];if j.is_null(){return None}
            let mut r=Recon::new(0,j["identifierSpace"].as_str().unwrap_or(""),j["schemaSpace"].as_str().unwrap_or(""));
            r.service=j["service"].as_str().unwrap().into();r.judgment=if j["j"]=="matched"{Judgment::Matched}else{Judgment::None};
            if j["m"].is_object(){r.matched=Some(candidate_from_json(&j["m"]))}Some(Arc::new(r))
        }).collect::<Vec<_>>();
        if cells.iter().any(Option::is_some){let c=&t.columns[ci];t.columns[ci]=Arc::new(Column{dict:c.dict.clone(),codes:c.codes.clone(),recon:Some(Arc::new(ReconCol{cells,..Default::default()}))})}
    }t
}
fn sheet(bytes:Vec<u8>)->Vec<Vec<String>>{
    let mut wb=calamine::open_workbook_auto_from_rs(Cursor::new(bytes)).unwrap();let names=wb.sheet_names();let range=wb.worksheet_range_at(0).unwrap().unwrap();
    std::iter::once(names).chain(range.rows().map(|row|row.iter().map(|c|match c{Data::Float(f)if f.fract()==0.0=>format!("{}",*f as i64),Data::DateTime(d)=>d.as_datetime().map_or(d.to_string(),|d|d.to_string()),c=>c.to_string()}).collect())).collect()
}
#[test]
fn ordered_custom_text_html_and_workbooks_match_official_reference(){
    let f=fixture();let t=table(&f);let rows=(0..t.rows as u32).collect::<Vec<_>>();
    for case in f["cases"].as_array().unwrap().iter().filter(|c|c["differenceKind"]!="date-controls"){
        let got=export(&t,&rows,case["format"].as_str().unwrap(),&case["params"],"custom-export").unwrap();
        if let Some(base64)=case["outputBase64"].as_str(){let want=base64::engine::general_purpose::STANDARD.decode(base64).unwrap();let want=sheet(want);let mut got=sheet(got);
            if case["differenceKind"]=="ods-time" { let offset=if case["params"]["options"]["outputColumnHeaders"]==true{2}else{1};assert!(got[offset][2].contains("03:04:05.123"));for (g,w) in got.iter_mut().zip(&want).skip(offset) {if !w[2].is_empty(){assert!(g[2].starts_with(&format!("{}T",w[2])));g[2]=w[2].clone();}} }
            assert_eq!(got,want,"{}",case["name"])}
        else{assert_eq!(String::from_utf8(got).unwrap(),case["outputText"].as_str().unwrap(),"{}",case["name"])}
    }
}
#[test]
fn requested_dates_work_where_openrefine_ignores_its_own_controls(){
    let f=fixture();let t=table(&f);let rows=[0,1,2];
    for case in f["cases"].as_array().unwrap().iter().filter(|c|c["differenceKind"]=="date-controls"){
        let got=export(&t,&rows,case["format"].as_str().unwrap(),&case["params"],"custom-export").unwrap();
        if case["format"]=="xlsx"{let got=sheet(got);assert_eq!(got[2][0],"2024/01/02 03:04:05");let want=sheet(base64::engine::general_purpose::STANDARD.decode(case["outputBase64"].as_str().unwrap()).unwrap());assert_ne!(got,want)}
        else{let got=String::from_utf8(got).unwrap();assert!(got.contains("2024/01/02 03:04:05"));assert!(case["outputText"].as_str().unwrap().contains("2024-01-02T03:04:05.123Z"));assert_ne!(got,case["outputText"].as_str().unwrap())}
    }
}
#[test]
fn same_text_with_different_recons_is_row_aware_and_backend_preference_supplies_links(){
    let f=fixture();let original=table(&f);let t=original.pick_rows(&[Some(0),Some(1),Some(0),Some(1),Some(0),Some(1)]);let rows=(0..6).collect::<Vec<_>>();
    let params=json!({"options":{"columns":[{"name":"name","reconSettings":{"output":"entity-id"}}]}});
    assert_eq!(String::from_utf8(export(&t,&rows,"csv",&params,"test").unwrap()).unwrap(),"name\r\nQ1\r\nQ3\r\nQ1\r\nQ3\r\nQ1\r\nQ3\r\n");
    let mut s=Session::with_store(Store::open(dir()).unwrap());s.project=Some(Project::new("test",t));
    dispatch(&mut s,"set_preference",json!({"name":"reconciliation.standardServices","value":[{"url":"https://recon.example.test/svc","view":{"url":"https://preferred.example/entity/{{id}}"}}]})).unwrap();
    let html=dispatch(&mut s,"export",json!({"format":"html","params":params,"limit":10})).unwrap()["data"].as_str().unwrap().to_string();
    assert!(html.contains("https://preferred.example/entity/Q1"));assert!(html.contains("https://preferred.example/entity/Q3"));
}
#[test]
fn typed_reconciled_cells_follow_text_vs_workbook_contract(){
    let f=fixture();let mut t=table(&f);let recon=t.columns[0].recon(0).unwrap().clone();
    for ci in [1,2,3]{let c=&t.columns[ci];t.columns[ci]=Arc::new(Column{dict:c.dict.clone(),codes:c.codes.clone(),recon:Some(Arc::new(ReconCol{cells:vec![Some(recon.clone()),None,None],..Default::default()}))})}
    for name in ["when","n","alive"]{
        let params=json!({"options":{"columns":[{"name":name,"reconSettings":{"output":"entity-id"}}]}});
        let csv=String::from_utf8(export(&t,&[0],"csv",&params,"test").unwrap()).unwrap();assert!(csv.contains("Q1"),"text output must use selected recon text even on typed cells");
        let data=sheet(export(&t,&[0],"xlsx",&params,"test").unwrap());assert_ne!(data[2][0],"Q1","OR's workbook serializer retains original numeric/bool/date values");
    }
}
#[test]
fn invalid_custom_settings_are_rejected_before_any_output_file_is_touched(){
    let f=fixture();let t=table(&f);let path=dir().join("existing");std::fs::write(&path,b"preserve").unwrap();
    for format in ["csv","tsv","*sv","html","xlsx","xls","ods"]{
        for pattern in ["yyyy 'o''clock'","SSSSSS","MMMMM","yyyyyyyy","yyyy Z","yyyy 'unfinished"]{
            let params=json!({"options":{"columns":[{"name":"when","dateSettings":{"format":"custom","custom":pattern}}]}});
            assert!(export_file(&t,&[0],format,&params,"test",&path).is_err(),"{format}: {pattern}");assert_eq!(std::fs::read(&path).unwrap(),b"preserve");
        }
        assert!(export_file(&t,&[0],format,&json!({"options":{"columns":[{"name":"missing"}]}}),"test",&path).is_err());
    }
    let expected=chrono::Utc.from_utc_datetime(&dates::parse_default("2024-01-02T03:04:05.123Z").unwrap()).with_timezone(&chrono::Local).to_rfc3339_opts(chrono::SecondsFormat::AutoSi,true);
    let got=String::from_utf8(export(&t,&[0],"csv",&json!({"options":{"columns":[{"name":"when","dateSettings":{"useLocalTimeZone":true}}]}}),"test").unwrap()).unwrap();
    assert!(got.contains(&expected));
}
use chrono::TimeZone;
