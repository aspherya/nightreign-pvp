//! Tee output: everything goes to stdout and into a buffer that is written to `dist\nrscan\*.txt` at the end.

use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

static BUF: Mutex<Vec<String>> = Mutex::new(Vec::new());

pub fn say(s: impl AsRef<str>) {
    let s = s.as_ref();
    println!("{s}");
    if let Ok(mut b) = BUF.lock() {
        b.push(s.to_string());
    }
}

/// Take everything said so far (and clear the buffer).
pub fn take() -> String {
    match BUF.lock() {
        Ok(mut b) => {
            let s = b.join("\n");
            b.clear();
            s
        }
        Err(_) => String::new(),
    }
}

/// Days since 1970-01-01 -> (year, month, day), proleptic Gregorian (Hinnant's civil_from_days).
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

pub fn utc_stamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let (y, mo, d) = civil_from_days(secs.div_euclid(86_400));
    let t = secs.rem_euclid(86_400);
    format!(
        "{y:04}{mo:02}{d:02}T{:02}{:02}{:02}Z",
        t / 3600,
        (t / 60) % 60,
        t % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn civil() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        // unix time 1791694348 is in the FS-0 run 9 log header: 2026-10-11
        assert_eq!(civil_from_days(1_791_694_348 / 86_400), (2026, 10, 11));
    }
}
