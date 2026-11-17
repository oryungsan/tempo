//! 월드타임 — 도시 목록 · 시간대 계산 (IANA 시간대 = chrono-tz, 서머타임 자동)
use chrono::{DateTime, Datelike, NaiveDate, Offset, TimeZone, Timelike, Utc};
use chrono_tz::Tz;

pub struct City {
    pub tz: &'static str,
    pub name: &'static str,
    pub country: &'static str,
}

const fn c(tz: &'static str, name: &'static str, country: &'static str) -> City {
    City { tz, name, country }
}

/// 고르기 쉬운 도시들 (그 밖의 시간대는 영문 이름으로 검색)
pub const CITIES: &[City] = &[
    c("Asia/Seoul", "서울", "대한민국"),
    c("Asia/Tokyo", "도쿄", "일본"),
    c("Asia/Shanghai", "베이징 · 상하이", "중국"),
    c("Asia/Hong_Kong", "홍콩", "중국"),
    c("Asia/Taipei", "타이베이", "대만"),
    c("Asia/Manila", "마닐라", "필리핀"),
    c("Asia/Singapore", "싱가포르", "싱가포르"),
    c("Asia/Kuala_Lumpur", "쿠알라룸푸르", "말레이시아"),
    c("Asia/Bangkok", "방콕", "태국"),
    c("Asia/Ho_Chi_Minh", "호찌민 · 하노이", "베트남"),
    c("Asia/Jakarta", "자카르타", "인도네시아"),
    c("Asia/Makassar", "발리", "인도네시아"),
    c("Asia/Kolkata", "뉴델리 · 뭄바이", "인도"),
    c("Asia/Kathmandu", "카트만두", "네팔"),
    c("Asia/Dhaka", "다카", "방글라데시"),
    c("Asia/Tashkent", "타슈켄트", "우즈베키스탄"),
    c("Asia/Almaty", "알마티", "카자흐스탄"),
    c("Asia/Ulaanbaatar", "울란바토르", "몽골"),
    c("Asia/Vladivostok", "블라디보스토크", "러시아"),
    c("Asia/Dubai", "두바이", "아랍에미리트"),
    c("Asia/Riyadh", "리야드", "사우디아라비아"),
    c("Asia/Tehran", "테헤란", "이란"),
    c("Asia/Jerusalem", "예루살렘", "이스라엘"),
    c("Europe/Istanbul", "이스탄불", "튀르키예"),
    c("Europe/Moscow", "모스크바", "러시아"),
    c("Europe/London", "런던", "영국"),
    c("Europe/Dublin", "더블린", "아일랜드"),
    c("Europe/Lisbon", "리스본", "포르투갈"),
    c("Europe/Paris", "파리", "프랑스"),
    c("Europe/Berlin", "베를린", "독일"),
    c("Europe/Madrid", "마드리드", "스페인"),
    c("Europe/Rome", "로마", "이탈리아"),
    c("Europe/Amsterdam", "암스테르담", "네덜란드"),
    c("Europe/Zurich", "취리히", "스위스"),
    c("Europe/Vienna", "빈", "오스트리아"),
    c("Europe/Prague", "프라하", "체코"),
    c("Europe/Stockholm", "스톡홀름", "스웨덴"),
    c("Europe/Helsinki", "헬싱키", "핀란드"),
    c("Europe/Athens", "아테네", "그리스"),
    c("Africa/Cairo", "카이로", "이집트"),
    c("Africa/Johannesburg", "요하네스버그", "남아프리카 공화국"),
    c("Africa/Nairobi", "나이로비", "케냐"),
    c("America/New_York", "뉴욕", "미국 동부"),
    c("America/Chicago", "시카고", "미국 중부"),
    c("America/Denver", "덴버", "미국 산악"),
    c("America/Los_Angeles", "로스앤젤레스 · 샌프란시스코", "미국 서부"),
    c("America/Anchorage", "앵커리지", "미국 알래스카"),
    c("Pacific/Honolulu", "호놀룰루", "미국 하와이"),
    c("Pacific/Guam", "괌", "미국령"),
    c("America/Toronto", "토론토", "캐나다"),
    c("America/Vancouver", "밴쿠버", "캐나다"),
    c("America/Mexico_City", "멕시코시티", "멕시코"),
    c("America/Sao_Paulo", "상파울루", "브라질"),
    c("America/Argentina/Buenos_Aires", "부에노스아이레스", "아르헨티나"),
    c("America/Lima", "리마", "페루"),
    c("America/Bogota", "보고타", "콜롬비아"),
    c("Australia/Sydney", "시드니", "호주"),
    c("Australia/Melbourne", "멜버른", "호주"),
    c("Australia/Brisbane", "브리즈번", "호주"),
    c("Australia/Perth", "퍼스", "호주"),
    c("Pacific/Auckland", "오클랜드", "뉴질랜드"),
    c("UTC", "협정 세계시", "UTC"),
];

pub fn city(tz: &str) -> Option<&'static City> {
    CITIES.iter().find(|c| c.tz == tz)
}

/// 표시 이름: 직접 정한 이름 → 도시 목록 이름 → 시간대 이름의 마지막 조각
pub fn display_name(tz: &str, label: &str) -> String {
    if !label.trim().is_empty() {
        return label.trim().to_string();
    }
    match city(tz) {
        Some(c) => c.name.to_string(),
        None => tz.rsplit('/').next().unwrap_or(tz).replace('_', " "),
    }
}

pub fn country(tz: &str) -> String {
    match city(tz) {
        Some(c) => c.country.to_string(),
        None => tz.split('/').next().unwrap_or("").to_string(),
    }
}

pub fn parse(tz: &str) -> Option<Tz> {
    tz.parse::<Tz>().ok()
}

/// 검색 — 한글 도시·나라 이름 또는 영문 시간대 이름 (빈 검색어 = 도시 목록 전체)
pub fn search(q: &str) -> Vec<(String, String, String)> {
    let q = q.trim().to_lowercase();
    let mut out: Vec<(String, String, String)> = CITIES
        .iter()
        .filter(|c| q.is_empty() || c.name.to_lowercase().contains(&q) || c.country.to_lowercase().contains(&q) || c.tz.to_lowercase().contains(&q))
        .map(|c| (c.tz.to_string(), c.name.to_string(), c.country.to_string()))
        .collect();
    if q.chars().count() >= 2 {
        let key = q.replace(' ', "_");
        for t in chrono_tz::TZ_VARIANTS.iter() {
            let n = t.name();
            if n.to_lowercase().contains(&key) && !out.iter().any(|o| o.0 == n) {
                out.push((n.to_string(), display_name(n, ""), country(n)));
                if out.len() >= 40 {
                    break;
                }
            }
        }
    }
    out
}

#[derive(Clone, Debug, PartialEq)]
pub struct CityTime {
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    pub date: NaiveDate,
    /// 내 PC 날짜와 비교 (-1 = 어제, 1 = 내일)
    pub day_diff: i64,
    /// 내 PC 시각과의 차이 (분)
    pub diff_min: i32,
    /// UTC 와의 차이 (분)
    pub utc_min: i32,
}

/// `utc` 순간의 그 도시 시각. `local_offset_min` = 내 PC 의 UTC 차이 (분)
pub fn at(tz: &str, utc: DateTime<Utc>, local_offset_min: i32) -> Option<CityTime> {
    let z = parse(tz)?;
    let t = z.from_utc_datetime(&utc.naive_utc());
    let utc_min = t.offset().fix().local_minus_utc() / 60;
    let local_date = (utc + chrono::Duration::minutes(local_offset_min as i64)).date_naive();
    Some(CityTime {
        hour: t.hour(),
        minute: t.minute(),
        second: t.second(),
        date: t.date_naive(),
        day_diff: (t.date_naive() - local_date).num_days(),
        diff_min: utc_min - local_offset_min,
        utc_min,
    })
}

/// 내 PC 의 UTC 차이 (분) — 그 순간 기준 (서머타임 반영)
pub fn local_offset_min(utc: DateTime<Utc>) -> i32 {
    chrono::Local.from_utc_datetime(&utc.naive_utc()).offset().fix().local_minus_utc() / 60
}

/// "+14시간" · "-9시간 30분" · "같은 시각"
pub fn diff_label(min: i32) -> String {
    if min == 0 {
        return "같은 시각".into();
    }
    let sign = if min > 0 { "+" } else { "-" };
    let a = min.unsigned_abs();
    if a % 60 == 0 {
        format!("{sign}{}시간", a / 60)
    } else if a < 60 {
        format!("{sign}{}분", a)
    } else {
        format!("{sign}{}시간 {}분", a / 60, a % 60)
    }
}

/// "UTC+9" · "UTC+5:30" · "UTC-3"
pub fn utc_label(min: i32) -> String {
    let sign = if min < 0 { "-" } else { "+" };
    let a = min.unsigned_abs();
    if a % 60 == 0 {
        format!("UTC{sign}{}", a / 60)
    } else {
        format!("UTC{sign}{}:{:02}", a / 60, a % 60)
    }
}

pub fn day_label(diff: i64) -> &'static str {
    match diff {
        0 => "오늘",
        1 => "내일",
        -1 => "어제",
        d if d > 1 => "모레",
        _ => "그제",
    }
}

/// 낮(6~18시)인가
pub fn daytime(hour: u32) -> bool {
    (6..18).contains(&hour)
}

pub const WD: [&str; 7] = ["월", "화", "수", "목", "금", "토", "일"];

pub fn weekday_ko(d: NaiveDate) -> &'static str {
    WD[d.weekday().num_days_from_monday() as usize]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offsets_and_dst() {
        // 2026-07-01 03:00 UTC — 뉴욕 서머타임(UTC-4), 서울 UTC+9
        let utc = Utc.with_ymd_and_hms(2026, 7, 1, 3, 0, 0).unwrap();
        let ny = at("America/New_York", utc, 540).unwrap();
        assert_eq!((ny.hour, ny.utc_min, ny.diff_min, ny.day_diff), (23, -240, -780, -1));
        assert_eq!(diff_label(ny.diff_min), "-13시간");
        // 겨울엔 UTC-5
        let w = at("America/New_York", Utc.with_ymd_and_hms(2026, 1, 15, 3, 0, 0).unwrap(), 540).unwrap();
        assert_eq!(w.utc_min, -300);
        let del = at("Asia/Kolkata", utc, 540).unwrap();
        assert_eq!((del.hour, del.minute), (8, 30));
        assert_eq!(diff_label(del.diff_min), "-3시간 30분");
        assert_eq!(utc_label(del.utc_min), "UTC+5:30");
        assert_eq!(utc_label(-180), "UTC-3");
        let akl = at("Pacific/Auckland", Utc.with_ymd_and_hms(2026, 7, 1, 14, 0, 0).unwrap(), 540).unwrap();
        assert_eq!((akl.hour, akl.day_diff), (2, 1));
    }

    #[test]
    fn cities_valid_and_search() {
        for c in CITIES {
            assert!(parse(c.tz).is_some(), "{}", c.tz);
        }
        assert_eq!(search("뉴욕")[0].0, "America/New_York");
        assert!(search("reykjavik").iter().any(|r| r.0 == "Atlantic/Reykjavik"));
        assert_eq!(display_name("Atlantic/Reykjavik", ""), "Reykjavik");
        assert_eq!(display_name("Asia/Seoul", " 본가 "), "본가");
    }
}
