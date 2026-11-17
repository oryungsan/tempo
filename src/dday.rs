//! 디데이 · 카운트다운 계산 (화면과 분리 — 시험 가능하게)
use crate::model::{Dday, DdayMode};
use chrono::{Datelike, Months, NaiveDate, NaiveDateTime};

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

/// [from, to] 안의 「그날」들 — (이름, 날짜). 매년 = 해마다, 그날부터 = 100일 단위 · 주년
pub fn events(d: &Dday, from: NaiveDate, to: NaiveDate) -> Vec<(String, NaiveDate)> {
    let Some(date) = d.date_nd() else { return vec![] };
    let mut v = Vec::new();
    match d.mode {
        DdayMode::Until => {
            if date >= from && date <= to {
                v.push(("D-Day".to_string(), date));
            }
        }
        DdayMode::Yearly => {
            for y in from.year().max(date.year())..=to.year() {
                let e = on_year(date, y);
                if e >= from && e <= to {
                    let k = y - date.year();
                    v.push((if k > 0 { format!("{k}번째") } else { "그날".into() }, e));
                }
            }
        }
        DdayMode::Since => {
            // N일째 = 시작 + (N-1)일
            let first = ((from - date).num_days() + 1).max(1);
            let mut n = (first + 99) / 100 * 100;
            loop {
                let e = date + chrono::Duration::days(n - 1);
                if e > to {
                    break;
                }
                v.push((format!("{n}일"), e));
                n += 100;
            }
            for y in (from.year() - date.year()).max(1)..=(to.year() - date.year()).max(1) {
                if let Some(e) = date.checked_add_months(Months::new(12 * y as u32)) {
                    if e >= from && e <= to {
                        v.push((format!("{y}주년"), e));
                    }
                }
            }
            v.sort_by_key(|x| x.1);
        }
    }
    v
}

#[derive(Clone, Debug, PartialEq)]
pub struct Alert {
    /// 「그날 날짜/며칠 전」 — 저장해서 두 번 안 울리게
    pub key: String,
    pub at: NaiveDateTime,
    /// 그날 이름 (D-Day · 100일 · 37번째)
    pub label: String,
    pub date: NaiveDate,
    /// 며칠 전 알림인가 (0 = 그날)
    pub before: u32,
}

/// 지금 울려야 할 알림들 — 울릴 시각이 지났고 12시간 안 (컴퓨터가 꺼져 있었어도 12시간 안이면 울림)
pub fn due_alerts(d: &Dday, now: NaiveDateTime) -> Vec<Alert> {
    if !d.notify {
        return vec![];
    }
    let t = d.alert_naive();
    let mut offs: Vec<u32> = vec![0];
    offs.extend(d.remind.iter().copied().filter(|n| *n > 0));
    let max = *offs.iter().max().unwrap_or(&0) as i64;
    let today = now.date();
    let mut out = Vec::new();
    for (label, e) in events(d, today - chrono::Duration::days(1), today + chrono::Duration::days(max + 1)) {
        for &n in &offs {
            let at = (e - chrono::Duration::days(n as i64)).and_time(t);
            if at <= now && now - at < chrono::Duration::hours(12) {
                out.push(Alert { key: format!("{e}/{n}"), at, label: label.clone(), date: e, before: n });
            }
        }
    }
    out
}

/// 하루 안으로 다가온 그날 — (이름, 그날 0시까지 남은 초). 화면에 초 단위로
pub fn soon(d: &Dday, i: &DdayInfo, now: NaiveDateTime) -> Option<(String, i64)> {
    let (label, date) = match (&i.next, d.mode) {
        (Some((n, dt, 1)), DdayMode::Since) => (n.clone(), *dt),
        (_, DdayMode::Since) => return None,
        _ if i.days == 1 => ("D-Day".to_string(), i.target),
        _ => return None,
    };
    let secs = (date.and_hms_opt(0, 0, 0)? - now).num_seconds();
    (secs > 0 && secs <= 86400).then(|| (format!("{label}까지"), secs))
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

    #[test]
    fn alerts_day_and_before() {
        let mut d = dd(nd(2026, 12, 10), DdayMode::Until);
        d.remind = vec![7, 1];
        let at = |y, m, dd_, h, mi| nd(y, m, dd_).and_hms_opt(h, mi, 0).unwrap();
        // 7일 전 09:00 이 지나면 울림, 그 전엔 안 울림
        assert!(due_alerts(&d, at(2026, 12, 3, 8, 59)).is_empty());
        let a = due_alerts(&d, at(2026, 12, 3, 9, 0));
        assert_eq!((a.len(), a[0].before, a[0].key.as_str()), (1, 7, "2026-12-10/7"));
        // 그날 09:30 — 그날 알림만 (1일 전 것은 12시간 넘게 지남)
        let a = due_alerts(&d, at(2026, 12, 10, 9, 30));
        assert_eq!((a.len(), a[0].before, a[0].label.as_str()), (1, 0, "D-Day"));
        // 22시 넘으면 12시간 지나 조용히 넘김
        assert!(due_alerts(&d, at(2026, 12, 10, 21, 1)).is_empty());
        d.notify = false;
        assert!(due_alerts(&d, at(2026, 12, 10, 9, 30)).is_empty());
    }

    #[test]
    fn alerts_since_and_yearly() {
        let at = |y, m, dd_, h| nd(y, m, dd_).and_hms_opt(h, 0, 0).unwrap();
        let s = dd(nd(2026, 1, 1), DdayMode::Since);
        assert_eq!(due_alerts(&s, at(2026, 4, 10, 10))[0].label, "100일");
        assert_eq!(due_alerts(&s, at(2027, 1, 1, 10))[0].label, "1주년");
        assert!(due_alerts(&s, at(2026, 4, 11, 10)).is_empty());
        let mut b = dd(nd(1990, 3, 5), DdayMode::Yearly);
        b.remind = vec![3];
        assert_eq!(due_alerts(&b, at(2027, 3, 2, 10))[0].key, "2027-03-05/3");
        assert_eq!(due_alerts(&b, at(2027, 3, 5, 10))[0].label, "37번째");
    }

    #[test]
    fn soon_counts_to_midnight() {
        let d = dd(nd(2026, 12, 10), DdayMode::Until);
        let now = nd(2026, 12, 9).and_hms_opt(10, 35, 55).unwrap();
        let i = info(&d, now.date()).unwrap();
        assert_eq!(soon(&d, &i, now), Some(("D-Day까지".into(), 13 * 3600 + 24 * 60 + 5)));
        let i2 = info(&d, nd(2026, 12, 8)).unwrap();
        assert_eq!(soon(&d, &i2, nd(2026, 12, 8).and_hms_opt(10, 0, 0).unwrap()), None);
        let s = dd(nd(2026, 1, 1), DdayMode::Since);
        let now = nd(2026, 4, 9).and_hms_opt(23, 0, 0).unwrap();
        let i = info(&s, now.date()).unwrap();
        assert_eq!(soon(&s, &i, now), Some(("100일까지".into(), 3600)));
    }

}
