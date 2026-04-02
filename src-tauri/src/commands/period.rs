use chrono::{Datelike, Local, NaiveDate};

pub(crate) fn parse_bucket_start_date(sort_key: &str) -> Result<NaiveDate, chrono::ParseError> {
    NaiveDate::parse_from_str(sort_key, "%Y-%m-%d")
        .or_else(|_| NaiveDate::parse_from_str(&format!("{sort_key}-01"), "%Y-%m-%d"))
}

/// Normalize a (base_year, base_month) pair shifted by `offset` months into a valid (year, month).
pub(crate) fn resolve_month_offset(base_year: i32, base_month: u32, offset: i32) -> (i32, u32) {
    let mut y = base_year;
    let mut m = base_month as i32 + offset;
    while m <= 0 {
        y -= 1;
        m += 12;
    }
    while m > 12 {
        y += 1;
        m -= 12;
    }
    (y, m as u32)
}

/// Return the first day of the month following (year, month).
pub(crate) fn first_of_next_month(year: i32, month: u32) -> NaiveDate {
    if month == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1).unwrap()
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1).unwrap()
    }
}

pub(crate) fn compute_date_bounds(period: &str, offset: i32) -> Option<(NaiveDate, NaiveDate)> {
    let now = Local::now();
    let today = now.date_naive();
    match period {
        "5h" => Some((today, today + chrono::Duration::days(1))),
        "day" => {
            let target = today + chrono::Duration::days(offset as i64);
            Some((target, target + chrono::Duration::days(1)))
        }
        "week" => {
            let current_monday =
                today - chrono::Duration::days(now.weekday().num_days_from_monday() as i64);
            let target_monday = current_monday + chrono::Duration::days((offset * 7) as i64);
            Some((target_monday, target_monday + chrono::Duration::days(7)))
        }
        "month" => {
            let (y, m) = resolve_month_offset(now.year(), now.month(), offset);
            let first = NaiveDate::from_ymd_opt(y, m, 1).unwrap();
            Some((first, first_of_next_month(y, m)))
        }
        "year" => {
            let ty = now.year() + offset;
            let first = NaiveDate::from_ymd_opt(ty, 1, 1).unwrap();
            Some((first, NaiveDate::from_ymd_opt(ty + 1, 1, 1).unwrap()))
        }
        _ => None,
    }
}

pub(crate) fn format_day_label(date: NaiveDate) -> String {
    date.format("%B %-d, %Y").to_string()
}

pub(crate) fn format_week_label(monday: NaiveDate, sunday: NaiveDate) -> String {
    if monday.year() != sunday.year() {
        format!(
            "{} \u{2013} {}",
            monday.format("%b %-d, %Y"),
            sunday.format("%b %-d, %Y")
        )
    } else if monday.month() != sunday.month() {
        format!(
            "{} \u{2013} {}",
            monday.format("%b %-d"),
            sunday.format("%b %-d, %Y")
        )
    } else {
        format!(
            "{} \u{2013} {}",
            monday.format("%b %-d"),
            sunday.format("%-d, %Y")
        )
    }
}

pub(crate) fn format_month_label(first_of_month: NaiveDate) -> String {
    first_of_month.format("%B %Y").to_string()
}

pub(crate) fn format_year_label(year: i32) -> String {
    year.to_string()
}

pub(crate) fn month_offset_from_now(year: i32, month: u32) -> i32 {
    let now = Local::now();
    (year - now.year()) * 12 + month as i32 - now.month() as i32
}
