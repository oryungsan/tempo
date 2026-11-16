//! 알람 다음 울릴 시각 계산
use crate::model::AlarmDef;
use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, TimeZone};

fn at(date: NaiveDate, h: u32, m: u32) -> Option<DateTime<Local>> {
    let ndt = date.and_hms_opt(h, m, 0)?;
    Local.from_local_datetime(&ndt).earliest()
}

/// `after` 보다 뒤의 첫 울림 시각
pub fn next_fire(a: &AlarmDef, after: DateTime<Local>) -> Option<DateTime<Local>> {
    let once = !a.days.iter().any(|d| *d);
    for k in 0..8 {
        let date = after.date_naive() + Duration::days(k);
        let wd = date.weekday().num_days_from_monday() as usize;
        if !once && !a.days[wd] {
            continue;
        }
        if let Some(t) = at(date, a.hour, a.minute) {
            if t > after {
                return Some(t);
            }
        }
    }
    None
}

pub fn days_label(days: &[bool; 7]) -> String {
    const N: [&str; 7] = ["월", "화", "수", "목", "금", "토", "일"];
    let c = days.iter().filter(|d| **d).count();
    if c == 0 {
        return "한 번".into();
    }
    if c == 7 {
        return "매일".into();
    }
    if *days == [true, true, true, true, true, false, false] {
        return "평일".into();
    }
    if *days == [false, false, false, false, false, true, true] {
        return "주말".into();
    }
    days.iter().zip(N).filter(|(d, _)| **d).map(|(_, n)| n).collect::<Vec<_>>().join(" ")
}

/// "3시간 12분 후"
pub fn until_label(t: DateTime<Local>, now: DateTime<Local>) -> String {
    let s = (t - now).num_seconds().max(0);
    let (d, h, m) = (s / 86400, (s % 86400) / 3600, (s % 3600) / 60);
    if d > 0 {
        format!("{d}일 {h}시간 후")
    } else if h > 0 {
        format!("{h}시간 {m}분 후")
    } else if m > 0 {
        format!("{m}분 후")
    } else {
        "곧".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::AlarmDef;
    use chrono::Timelike;

    #[test]
    fn weekday_and_once() {
        // 2026-11-16 = 월요일
        let mon_10 = Local.with_ymd_and_hms(2026, 11, 16, 10, 0, 0).unwrap();
        let mut a = AlarmDef::new();
        a.hour = 9;
        a.minute = 0;
        a.days = [false, false, true, false, false, false, false]; // 수
        let n = next_fire(&a, mon_10).unwrap();
        assert_eq!((n.day(), n.hour()), (18, 9));
        a.days = [false; 7];
        let n = next_fire(&a, mon_10).unwrap();
        assert_eq!(n.day(), 17); // 오늘 9시는 지남 → 내일
        a.hour = 11;
        assert_eq!(next_fire(&a, mon_10).unwrap().day(), 16);
        a.days = [true; 7];
        assert_eq!(days_label(&a.days), "매일");
    }
}
