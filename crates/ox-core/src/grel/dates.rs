//! Dates as OpenRefine 3.10 shows them: UTC, offsets in the text are dropped, Java date patterns.
use chrono::{Datelike, Duration, Months, NaiveDate, NaiveDateTime, NaiveTime, Timelike};
use regex::Regex;
use std::sync::OnceLock;

/// toDate() without a format. Accepts ISO dates/times and common English month forms.
pub fn parse_default(s: &str) -> Option<NaiveDateTime> {
    static ISO: OnceLock<Regex> = OnceLock::new();
    let iso = ISO.get_or_init(|| {
        Regex::new(r"^(\d{4})-(\d{1,2})-(\d{1,2})(?:[T ](\d{1,2}):(\d{2})(?::(\d{2})(?:[.,](\d{1,9}))?)?)?\s*(?:Z|[+-]\d{2}:?\d{2}|UTC|GMT)?$").unwrap()
    });
    let s = s.trim();
    // a bare year or year-month is a date too ("0042" is 0042-01-01)
    if s.len() == 4 && s.bytes().all(|b| b.is_ascii_digit()) {
        return NaiveDate::from_ymd_opt(s.parse().ok()?, 1, 1).map(|d| d.and_time(NaiveTime::MIN));
    }
    if let Some((y, m)) = s.split_once('-').filter(|(y, m)| y.len() == 4 && (1..=2).contains(&m.len()) && y.bytes().chain(m.bytes()).all(|b| b.is_ascii_digit())) {
        return NaiveDate::from_ymd_opt(y.parse().ok()?, m.parse().ok()?, 1).map(|d| d.and_time(NaiveTime::MIN));
    }
    if let Some(c) = iso.captures(s) {
        let n = |i: usize| c.get(i).map_or(Some(0), |m| m.as_str().parse::<u32>().ok());
        let date = NaiveDate::from_ymd_opt(c[1].parse().ok()?, n(2)?, n(3)?)?;
        let nanos = c.get(7).map_or(0, |m| format!("{:0<9}", m.as_str()).parse().unwrap_or(0));
        return date.and_hms_nano_opt(n(4)?, n(5)?, n(6)?, nanos);
    }
    const FORMATS: &[&str] = &["%B %d, %Y", "%b %d, %Y", "%d %B %Y", "%d %b %Y", "%B %d %Y", "%Y/%m/%d", "%m/%d/%Y", "%Y.%m.%d"];
    for f in FORMATS {
        if let Ok(d) = NaiveDate::parse_from_str(s, f) {
            return Some(d.and_time(NaiveTime::MIN));
        }
    }
    for f in ["%Y/%m/%d %H:%M:%S", "%m/%d/%Y %H:%M:%S", "%Y/%m/%d %H:%M", "%m/%d/%Y %H:%M", "%B %d, %Y %H:%M:%S"] {
        if let Ok(d) = NaiveDateTime::parse_from_str(s, f) {
            return Some(d);
        }
    }
    None
}

/// Translate a Java SimpleDateFormat / DateTimeFormatter pattern to a chrono format string.
pub fn java_pattern(p: &str) -> String {
    let chars: Vec<char> = p.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\'' {
            i += 1;
            if chars.get(i) == Some(&'\'') {
                out.push('\'');
                i += 1;
                continue;
            }
            while i < chars.len() && chars[i] != '\'' {
                if chars[i] == '%' {
                    out.push('%');
                }
                out.push(chars[i]);
                i += 1;
            }
            i += 1;
            continue;
        }
        let mut n = 1;
        while chars.get(i + n) == Some(&c) {
            n += 1;
        }
        i += n;
        let spec = match (c, n) {
            ('y' | 'u', 2) => "%y",
            ('y' | 'u', _) => "%Y",
            ('M' | 'L', 1) => "%-m",
            ('M' | 'L', 2) => "%m",
            ('M' | 'L', 3) => "%b",
            ('M' | 'L', _) => "%B",
            ('d', 1) => "%-d",
            ('d', _) => "%d",
            ('D', _) => "%j",
            ('E', 4..) => "%A",
            ('E', _) => "%a",
            ('H', 1) => "%-H",
            ('H', _) => "%H",
            ('h', 1) => "%-I",
            ('h', _) => "%I",
            ('m', 1) => "%-M",
            ('m', _) => "%M",
            ('s', 1) => "%-S",
            ('s', _) => "%S",
            ('S', _) => "%3f",
            ('a', _) => "%p",
            ('X' | 'x' | 'Z' | 'z' | 'O', _) => "Z",
            ('%', _) => "%%",
            _ => {
                for _ in 0..n {
                    out.push(c);
                }
                continue;
            }
        };
        out.push_str(spec);
    }
    out
}

/// toDate(s, format): parse a prefix of `s` with a Java pattern, like SimpleDateFormat's lenient parse.
pub fn parse_with(s: &str, pattern: &str) -> Option<NaiveDateTime> {
    let f = java_pattern(pattern);
    let s = s.trim();
    if let Ok((d, _)) = NaiveDateTime::parse_and_remainder(s, &f) {
        return Some(d);
    }
    NaiveDate::parse_and_remainder(s, &f).ok().map(|(d, _)| d.and_time(NaiveTime::MIN))
}

pub fn format(d: &NaiveDateTime, pattern: &str) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    // an invalid pattern makes chrono's Display fail; fall back to the ISO form
    if write!(out, "{}", d.format(&java_pattern(pattern))).is_err() {
        return super::value::fmt_date(d);
    }
    out
}

fn unit(u: &str) -> &str {
    let u = u.to_ascii_lowercase();
    match u.trim_end_matches('s') {
        "year" | "y" => "year",
        "month" | "mo" => "month",
        "week" | "w" => "week",
        "day" | "d" => "day",
        "hour" | "h" => "hour",
        "minute" | "min" | "mi" => "minute",
        "second" | "sec" | "s" => "second",
        "millisecond" | "milli" | "ms" => "ms",
        "nano" | "nanosecond" | "n" => "nano",
        _ => "",
    }
}

pub fn inc(d: &NaiveDateTime, n: i64, u: &str) -> Option<NaiveDateTime> {
    match unit(u) {
        "year" => add_months(d, n.checked_mul(12)?),
        "month" => add_months(d, n),
        "week" => d.checked_add_signed(Duration::try_weeks(n)?),
        "day" => d.checked_add_signed(Duration::try_days(n)?),
        "hour" => d.checked_add_signed(Duration::try_hours(n)?),
        "minute" => d.checked_add_signed(Duration::try_minutes(n)?),
        "second" => d.checked_add_signed(Duration::try_seconds(n)?),
        "ms" => d.checked_add_signed(Duration::try_milliseconds(n)?),
        "nano" => d.checked_add_signed(Duration::nanoseconds(n)),
        _ => None,
    }
}

fn add_months(d: &NaiveDateTime, n: i64) -> Option<NaiveDateTime> {
    if n >= 0 {
        d.checked_add_months(Months::new(u32::try_from(n).ok()?))
    } else {
        d.checked_sub_months(Months::new(u32::try_from(-n).ok()?))
    }
}

/// diff(d1, d2, unit): whole units from d2 to d1 (ChronoUnit.between).
pub fn diff(a: &NaiveDateTime, b: &NaiveDateTime, u: &str) -> Option<i64> {
    let d = *a - *b;
    Some(match unit(u) {
        "year" => months_between(b, a) / 12,
        "month" => months_between(b, a),
        "week" => d.num_weeks(),
        "day" => d.num_days(),
        "hour" => d.num_hours(),
        "minute" => d.num_minutes(),
        "second" => d.num_seconds(),
        "ms" => d.num_milliseconds(),
        "nano" => d.num_nanoseconds()?,
        _ => return None,
    })
}

fn months_between(from: &NaiveDateTime, to: &NaiveDateTime) -> i64 {
    let mut m = (to.year() as i64 - from.year() as i64) * 12 + to.month() as i64 - from.month() as i64;
    let (f, t) = ((from.day(), from.time()), (to.day(), to.time()));
    if m > 0 && t < f {
        m -= 1;
    } else if m < 0 && t > f {
        m += 1;
    }
    m
}

pub enum Part {
    Num(i64),
    Text(&'static str),
}

pub fn part(d: &NaiveDateTime, p: &str) -> Option<Part> {
    use Part::*;
    Some(match p {
        "years" | "year" => Num(d.year() as i64),
        "months" | "month" => Num(d.month() as i64),
        "weeks" | "week" | "w" => Num(((d.day() - 1) / 7 + 1) as i64),
        "days" | "day" | "d" => Num(d.day() as i64),
        "weekday" => Text(match d.weekday() {
            chrono::Weekday::Mon => "MONDAY",
            chrono::Weekday::Tue => "TUESDAY",
            chrono::Weekday::Wed => "WEDNESDAY",
            chrono::Weekday::Thu => "THURSDAY",
            chrono::Weekday::Fri => "FRIDAY",
            chrono::Weekday::Sat => "SATURDAY",
            chrono::Weekday::Sun => "SUNDAY",
        }),
        "hours" | "hour" | "h" => Num(d.hour() as i64),
        "minutes" | "minute" | "min" => Num(d.minute() as i64),
        "seconds" | "sec" | "s" => Num(d.second() as i64),
        "milliseconds" | "ms" | "S" => Num((d.nanosecond() / 1_000_000) as i64),
        "nanos" => Num(d.nanosecond() as i64),
        "time" => Num(d.and_utc().timestamp_millis()),
        _ => return None,
    })
}
