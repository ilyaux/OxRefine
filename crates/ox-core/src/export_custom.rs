use super::{ColumnChoice, TabularOptions};
use crate::{grel::{dates, value::fmt_date, Value}, recon::{Judgment, Recon}, table::{Column, Table}, Result};
use chrono::{Local, SecondsFormat, TimeZone, Utc};
use serde::Deserialize;
use serde_json::Value as J;

#[derive(Clone, Deserialize)]
#[serde(rename_all="camelCase",default)]
pub struct DateSettings { pub format: String, pub custom: String, pub use_local_time_zone: bool, pub omit_time: bool }
impl Default for DateSettings { fn default() -> Self { Self { format:"iso-8601".into(),custom:String::new(),use_local_time_zone:false,omit_time:false } } }
#[derive(Clone, Deserialize)]
#[serde(rename_all="camelCase",default)]
pub struct ReconSettings { pub output: String, pub blank_unmatched_cells: bool, pub link_to_entity_pages: bool }
impl Default for ReconSettings { fn default() -> Self { Self { output:"entity-name".into(),blank_unmatched_cells:false,link_to_entity_pages:true } } }

fn pattern(settings: &DateSettings) -> Result<Option<&str>> {
    Ok(Some(match settings.format.as_str() {
        "iso-8601" => return Ok(None),
        "locale-short" => if settings.omit_time { "M/d/yy" } else { "M/d/yy, h:mm a" },
        "locale-medium" => if settings.omit_time { "MMM d, yyyy" } else { "MMM d, yyyy, h:mm:ss a" },
        "locale-long" => if settings.omit_time { "MMMM d, yyyy" } else { "MMMM d, yyyy, h:mm:ss a" },
        "locale-full" => if settings.omit_time { "EEEE, MMMM d, yyyy" } else { "EEEE, MMMM d, yyyy, h:mm:ss a" },
        "custom" if !settings.custom.trim().is_empty() => &settings.custom,
        "custom" => return Err("A custom date format cannot be empty".into()),
        _ => return Err(format!("Unknown date format {}",settings.format)),
    }))
}
pub fn validate(t: &Table, options: &TabularOptions) -> Result<()> {
    for c in &options.columns {
        t.col(&c.name)?;
        if !matches!(c.recon_settings.output.as_str(),"entity-name"|"entity-id"|"cell-content") { return Err("Reconciliation output must be entity-name, entity-id or cell-content".into()); }
        if let Some(p)=pattern(&c.date_settings)? {
            if p.chars().count()>256 { return Err("A date pattern may contain at most 256 characters".into()); }
            if p.contains("''") { return Err("Escaped apostrophes are not supported in export date patterns. Use simple quoted literals.".into()); }
            let mut quoted=false; let mut chars=p.chars().peekable();
            while let Some(ch)=chars.next() {
                if ch=='\'' { if chars.peek()==Some(&'\'') { chars.next(); } else { quoted=!quoted; } }
                else if !quoted && ch.is_ascii_alphabetic() {
                    let mut count=1; while chars.peek()==Some(&ch) { chars.next(); count+=1; }
                    let supported=match ch { 'y'=>matches!(count,2|4),'u'=>count==4,'M'|'L'=>count<=4,'d'|'H'|'h'|'m'|'s'=>count<=2,'E'=>count<=4,'D'|'S'=>count==3,'a'=>count==1,_=>false };
                    if !supported { return Err(format!("Unsupported date token {}. Use yy/yyyy, M..MMMM, d/dd, E..EEEE, H/HH, h/hh, m/mm, s/ss, SSS, a and simple quoted literals.",ch.to_string().repeat(count))); }
                }
            }
            if quoted { return Err("Unclosed quote in custom date format".into()); }
            if chrono::format::StrftimeItems::new(&dates::java_pattern(p)).any(|item| matches!(item,chrono::format::Item::Error)) { return Err("Invalid date pattern".into()); }
        }
    }
    Ok(())
}
pub fn value(value: &Value, recon: Option<&Recon>, choice: Option<&ColumnChoice>, binary: bool) -> Value {
    let Some(choice)=choice else { return value.clone(); };
    let settings=&choice.recon_settings;
    let mut rendered=value.clone();
    let mut replaced_by_recon=false;
    if let Some(recon)=recon {
        if recon.judgment==Judgment::Matched {
            if let Some(m)=&recon.matched {
                rendered=match settings.output.as_str() { "entity-name"=>{replaced_by_recon=true;Value::Str(m.name.clone())},"entity-id"=>{replaced_by_recon=true;Value::Str(m.id.clone())},_=>rendered };
            }
        } else if settings.blank_unmatched_cells { return Value::Null; }
    }
    let ds=&choice.date_settings;
    if replaced_by_recon && !binary { return rendered; }
    if let Value::Date(date)=value {
        let custom=ds.format!="iso-8601" || ds.omit_time || ds.use_local_time_zone;
        if binary && !custom { return value.clone(); }
        let local=Utc.from_utc_datetime(date).with_timezone(&Local);
        let date=if ds.use_local_time_zone { local.naive_local() } else { *date };
        let text=if let Ok(Some(p))=pattern(ds) { dates::format(&date,p) }
            else if ds.omit_time { date.format("%Y-%m-%d").to_string() }
            else if ds.use_local_time_zone { local.to_rfc3339_opts(SecondsFormat::AutoSi,true) }
            else { fmt_date(&date) };
        return Value::Str(text);
    }
    if binary && matches!(value,Value::Int(_)|Value::Num(_)|Value::Bool(_)) { return value.clone(); }
    rendered
}
pub fn link(t: &Table, ci: usize, row: usize, choice: Option<&ColumnChoice>, params: &J) -> Option<String> {
    let value=t.columns[ci].get(row);
    let recon=t.columns[ci].recon(row);
    let raw=if let Some(recon)=recon.filter(|r|r.judgment==Judgment::Matched) {
        if !choice.map(|c|c.recon_settings.link_to_entity_pages).unwrap_or(false) { return None; }
        let id=&recon.matched.as_ref()?.id;
        let template=params["reconciliationServices"].as_array().into_iter().flatten().find(|s|s["url"].as_str()==Some(&recon.service)||s["service"].as_str()==Some(&recon.service))
            .and_then(|s|s["view"]["url"].as_str());
        if let Some(template)=template { template.replace("{{id}}",id) }
        else { format!("{}{}",recon.identifier_space.as_deref()?,id) }
    } else { value.as_str()?.to_string() };
    let url=url::Url::parse(&raw).ok()?;
    matches!(url.scheme(),"http"|"https"|"ftp").then_some(raw)
}

/// Workbooks already materialize their output. Only explicit custom options take this path;
/// ordinary exports retain their original types, layout and fast path.
pub fn project(t: &Table, rows: &[u32], options: &TabularOptions) -> Result<Table> {
    validate(t,options)?;
    let indices=super::columns(t,&options.columns)?;
    let mut cols=vec![Vec::new();indices.len()];
    for &row in rows {
        let values=indices.iter().enumerate().map(|(slot,&ci)| value(t.columns[ci].get(row as usize),t.columns[ci].recon(row as usize).map(|r|r.as_ref()),options.columns.get(slot),true)).collect::<Vec<_>>();
        if !options.output_blank_rows && values.iter().all(Value::is_null) { continue; }
        for (out,value) in cols.iter_mut().zip(values) { out.push(value); }
    }
    let n=cols.first().map_or(0,Vec::len);
    let budget = crate::storage::current_budget();
    Ok(Table::new(indices.iter().map(|&ci|t.names[ci].clone()).collect(),cols.into_iter().map(|values| Column::try_from_values(values, &budget)).collect::<Result<_>>()?,n))
}
