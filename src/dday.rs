//! 디데이 · 카운트다운 계산 (화면과 분리 — 시험 가능하게)
use crate::model::{Dday, DdayMode};
use chrono::{Datelike, Months, NaiveDate};

#[derive(Clone, Debug, PartialEq)]
pub struct DdayInfo {
    /// 큰 글자 — "D-23" · "D-Day" · "D+5" · "100일째"
    pub big: String,
    /// 보조 — "23일 남음" · "D+99" · "34번째"
    pub small: String,
    /// 셀 기준 날짜 (매년 = 다음 그날)
    pub target: NaiveDate,
    /// 기준 날짜 - 오늘 (일)
    pub days: i64,
    /// 다음 기념일 (이름, 날짜, 남은 날)
    pub next: Option<(String, NaiveDate, i64)>,
}

fn d_label(days: i64) -> String {
    match days {
        0 => "D-Day".into(),
        d if d > 0 => format!("D-{d}"),
        d => format!("D+{}", -d),
    }
}

/// 그해의 (월, 일) — 2월 29일은 평년엔 2월 28일
fn on_year(base: NaiveDate, year: i32) -> NaiveDate {
    NaiveDate::from_ymd_opt(year, base.month(), base.day()).unwrap_or_else(|| NaiveDate::from_ymd_opt(year, 2, 28).unwrap())
}

/// 그날부터 셀 때 다음 기념일 — 100일 단위 · 해마다
pub fn next_milestone(start: NaiveDate, today: NaiveDate) -> Option<(String, NaiveDate, i64)> {
    let mut best: Option<(String, NaiveDate)> = None;
    let mut consider = |name: String, d: NaiveDate| {
        if d >= today && best.as_ref().map(|b| d < b.1).unwrap_or(true) {
            best = Some((name, d));
        }
    };
    // 첫날 = 1일째 → N일째 = 시작 + (N-1)일
    let n_now = (today - start).num_days() + 1;
    let k = (n_now.max(1) + 99) / 100 * 100;
    for n in [k, k + 100] {
        consider(format!("{n}일"), start + chrono::Duration::days(n - 1));
    }
    let years = (today.year() - start.year()).max(1);
    for y in [years, years + 1] {
        if let Some(d) = start.checked_add_months(Months::new(12 * y as u32)) {
            consider(format!("{y}주년"), d);
        }
    }
    best.map(|(n, d)| {
        let left = (d - today).num_days();
        (n, d, left)
    })
}

pub fn info(d: &Dday, today: NaiveDate) -> Option<DdayInfo> {
    let date = d.date_nd()?;
    Some(match d.mode {
        DdayMode::Until => {
            let days = (date - today).num_days();
            let small = match days {
                0 => String::new(),
                x if x > 0 => format!("{x}일 남음"),
                x => format!("{}일 지남", -x),
            };
            DdayInfo { big: d_label(days), small, target: date, days, next: None }
        }
        DdayMode::Since => {
            let days = (date - today).num_days();
            if days > 0 {
                DdayInfo { big: d_label(days), small: "시작 전".into(), target: date, days, next: None }
            } else {
                let n = -days + 1;
                DdayInfo { big: format!("{n}일째"), small: d_label(days), target: date, days, next: next_milestone(date, today) }
            }
        }
        DdayMode::Yearly => {
            let mut next = on_year(date, today.year());
            if next < today {
                next = on_year(date, today.year() + 1);
            }
            let days = (next - today).num_days();
            let k = next.year() - date.year();
            let small = if k > 0 { format!("{k}번째") } else { String::new() };
            DdayInfo { big: d_label(days), small, target: next, days, next: None }
        }
    })
}

/// 남은 초 → (일, 시, 분, 초)
pub fn split(secs: i64) -> (i64, i64, i64, i64) {
    let s = secs.abs();
    (s / 86400, (s % 86400) / 3600, (s % 3600) / 60, s % 60)
}

/// "38일 04:12:33" · "04:12:33" · "12:33"
pub fn remain_text(secs: i64) -> (String, String) {
    let (d, h, m, s) = split(secs);
    let clock = if d > 0 || h > 0 { format!("{h:02}:{m:02}:{s:02}") } else { format!("{m:02}:{s:02}") };
    (if d > 0 { format!("{d}일") } else { String::new() }, clock)
}

/// "3시간 12분 전" 같이 대략
pub fn ago_text(secs: i64) -> String {
    let (d, h, m, _) = split(secs);
    if d > 0 {
        format!("{d}일 {h}시간 전")
    } else if h > 0 {
        format!("{h}시간 {m}분 전")
    } else if m > 0 {
        format!("{m}분 전")
    } else {
        "방금".into()
    }
}

/// "2026년 12월 10일 (목)" — 올해면 연도 생략
pub fn date_ko(d: NaiveDate, today: NaiveDate) -> String {
    let wd = crate::world::weekday_ko(d);
    if d.year() == today.year() {
        format!("{}월 {}일 ({wd})", d.month(), d.day())
    } else {
        format!("{}년 {}월 {}일 ({wd})", d.year(), d.month(), d.day())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nd(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn dd(date: NaiveDate, mode: DdayMode) -> Dday {
        Dday::new("t", date, mode, "#5B8DEF")
    }

    #[test]
    fn until() {
        let today = nd(2026, 11, 17);
        assert_eq!(info(&dd(nd(2026, 12, 10), DdayMode::Until), today).unwrap().big, "D-23");
        assert_eq!(info(&dd(today, DdayMode::Until), today).unwrap().big, "D-Day");
        let past = info(&dd(nd(2026, 11, 12), DdayMode::Until), today).unwrap();
        assert_eq!((past.big.as_str(), past.small.as_str()), ("D+5", "5일 지남"));
    }

    #[test]
    fn since_counts_first_day_as_one() {
        let start = nd(2026, 1, 1);
        let i = info(&dd(start, DdayMode::Since), start).unwrap();
        assert_eq!(i.big, "1일째");
        // 100일째 = 시작 + 99일 = 4월 10일
        let i = info(&dd(start, DdayMode::Since), nd(2026, 4, 10)).unwrap();
        assert_eq!((i.big.as_str(), i.small.as_str()), ("100일째", "D+99"));
        assert_eq!(i.next, Some(("100일".into(), nd(2026, 4, 10), 0)));
        // 다음 날이면 다음 = 200일 (7월 19일)
        let i = info(&dd(start, DdayMode::Since), nd(2026, 4, 11)).unwrap();
        assert_eq!(i.next, Some(("200일".into(), nd(2026, 7, 19), 99)));
        // 1주년이 300일보다 앞서면 1주년이 아니라 가까운 쪽
        let i = info(&dd(start, DdayMode::Since), nd(2026, 12, 20)).unwrap();
        assert_eq!(i.next.unwrap().0, "1주년");
        // 시작 전
        assert_eq!(info(&dd(nd(2026, 12, 1), DdayMode::Since), nd(2026, 11, 28)).unwrap().big, "D-3");
    }

    #[test]
    fn yearly() {
        let b = dd(nd(1990, 3, 5), DdayMode::Yearly);
        let i = info(&b, nd(2026, 11, 17)).unwrap();
        assert_eq!((i.target, i.small.as_str()), (nd(2027, 3, 5), "37번째"));
        let i = info(&b, nd(2027, 3, 5)).unwrap();
        assert_eq!((i.big.as_str(), i.days), ("D-Day", 0));
        // 2월 29일 → 평년엔 2월 28일
        let leap = dd(nd(2024, 2, 29), DdayMode::Yearly);
        assert_eq!(info(&leap, nd(2026, 1, 1)).unwrap().target, nd(2026, 2, 28));
    }

    #[test]
    fn remain() {
        assert_eq!(remain_text(38 * 86400 + 4 * 3600 + 12 * 60 + 33), ("38일".into(), "04:12:33".into()));
        assert_eq!(remain_text(753), (String::new(), "12:33".into()));
        assert_eq!(ago_text(3 * 3600 + 12 * 60), "3시간 12분 전");
    }
}
