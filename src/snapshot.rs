//! Snapshot records: one scan as a JSON line, the latest by instant, the change since then per role.
//!
//! Stamps carry their offset, so text order is not time order — two records an hour apart can read
//! the same wall time on the night the clocks change. Records are compared by parsed instant only.

use std::collections::BTreeMap;

use jiff::{Timestamp, Zoned};
use serde_json::{Value, json};

use crate::report;
use crate::types::{Role, Scan};

const SCHEMA: u64 = 1;

/// The stamp of a record: the wall time in its zone, with its offset.
pub fn stamp(at: &Zoned) -> String {
    at.strftime("%Y-%m-%dT%H:%M:%S%:z").to_string()
}

/// The snapshot record of one scan, stamped with `at` and its offset.
pub fn record(scan: &Scan, at: &Zoned) -> Value {
    let matrix: Vec<Value> = report::matrix([scan])
        .into_iter()
        .map(|cell| json!([cell.language, cell.role.name(), cell.kind.name(), cell.lines]))
        .collect();
    let excluded: serde_json::Map<String, Value> =
        report::excluded([scan]).into_iter().map(|(reason, count)| (reason.name().to_owned(), json!(count))).collect();
    json!({"schema": SCHEMA, "at": stamp(at), "root": scan.root_text(), "matrix": matrix, "excluded": excluded})
}

/// Read the records of a snapshot file; a line that is no record is passed over.
pub fn parse<'line>(lines: impl IntoIterator<Item = &'line str>) -> Vec<Value> {
    lines
        .into_iter()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|record| instant(record).is_some())
        .collect()
}

fn instant(record: &Value) -> Option<Timestamp> {
    record.get("at")?.as_str()?.parse().ok()
}

/// The record of `root` with the latest instant, or None when there is none.
pub fn latest<'record>(records: &'record [Value], root: &str) -> Option<&'record Value> {
    records
        .iter()
        .filter(|record| record.get("root").and_then(Value::as_str) == Some(root))
        .max_by_key(|record| instant(record))
}

/// Lines gained or lost per role since `previous`, roles in the order of the role table.
pub fn delta(scan: &Scan, previous: &Value) -> Vec<(Role, i64)> {
    let mut now: BTreeMap<Role, i64> = BTreeMap::new();
    for cell in report::matrix([scan]) {
        *now.entry(cell.role).or_default() += cell.lines as i64;
    }
    let mut before: BTreeMap<Role, i64> = BTreeMap::new();
    for entry in previous.get("matrix").and_then(Value::as_array).into_iter().flatten() {
        let role = entry.get(1).and_then(Value::as_str).and_then(Role::from_name);
        if let (Some(role), Some(lines)) = (role, entry.get(3).and_then(Value::as_i64)) {
            *before.entry(role).or_default() += lines;
        }
    }
    Role::ALL
        .into_iter()
        .filter(|role| now.contains_key(role) || before.contains_key(role))
        .map(|role| {
            (role, now.get(&role).copied().unwrap_or_default() - before.get(&role).copied().unwrap_or_default())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::count;

    fn empty() -> Scan {
        count::assemble(PathBuf::from("repository"), [])
    }

    #[test]
    fn shall_36_the_latest_instant_wins_even_when_its_text_sorts_first() {
        let root = "C:/dev/repository";
        let summer = json!({"root": root, "at": "2026-10-25T02:30:00+02:00", "matrix": []});
        let winter = json!({"root": root, "at": "2026-10-25T02:30:00+01:00", "matrix": []});
        let other = json!({"root": "C:/dev/elsewhere", "at": "2026-12-24T12:00:00+01:00", "matrix": []});
        assert_eq!(latest(&[winter.clone(), summer.clone(), other], root), Some(&winter));
        assert_eq!(latest(&[summer, winter.clone()], root), Some(&winter));
    }

    #[test]
    fn delta_counts_roles_that_disappeared() {
        assert_eq!(delta(&empty(), &json!({"matrix": [["Python", "test", "code", 4]]})), [(Role::Test, -4)]);
    }

    #[test]
    fn a_record_stamps_the_instant_it_is_given() {
        let at: Zoned = "2026-10-19T15:20:00[Europe/Berlin]".parse().unwrap();
        assert_eq!(record(&empty(), &at)["at"], "2026-10-19T15:20:00+02:00");
        assert_eq!(record(&empty(), &at.with_time_zone(jiff::tz::TimeZone::UTC))["at"], "2026-10-19T13:20:00+00:00");
        let west = at.with_time_zone(jiff::tz::TimeZone::fixed(jiff::tz::offset(-5)));
        assert!(record(&empty(), &west)["at"].as_str().unwrap().ends_with("-05:00"));
    }

    #[test]
    fn a_line_that_is_no_record_is_passed_over() {
        let lines = ["not json", "{\"root\": \"x\"}", "{\"root\": \"x\", \"at\": \"2026-10-19T15:20:00+02:00\"}"];
        assert_eq!(parse(lines).len(), 1);
    }
}
