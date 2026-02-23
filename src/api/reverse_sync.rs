use std::collections::{HashMap, HashSet};

use anyhow::{Context, Result};
use chrono::NaiveDateTime;
use reqwest::{Client, header};

use crate::api::sync;

const VOLATILE_FIELDS: &[&str] = &["DTSTAMP", "SEQUENCE", "LAST-MODIFIED", "CREATED"];

#[derive(Debug)]
pub struct ReverseSyncStats {
    pub uploaded: usize,
    pub skipped: usize,
    pub deleted: usize,
    pub total: usize,
}

fn unfold_ics(text: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    for line in text.lines() {
        if (line.starts_with(' ') || line.starts_with('\t')) && !lines.is_empty() {
            if let Some(last) = lines.last_mut() {
                last.push_str(&line[1..]);
            }
        } else {
            lines.push(line.to_string());
        }
    }
    lines.join("\n")
}

fn normalize_vevent(vevent_data: &str) -> Vec<String> {
    let unfolded = unfold_ics(vevent_data);
    let mut lines: Vec<String> = unfolded
        .lines()
        .map(str::trim)
        .filter(|line| {
            !line.is_empty()
                && !VOLATILE_FIELDS.iter().any(|&field| {
                    line.starts_with(field)
                        && line
                            .as_bytes()
                            .get(field.len())
                            .is_some_and(|&b| b == b':' || b == b';')
                })
        })
        .map(String::from)
        .collect();
    lines.sort();
    lines
}

fn events_equal(existing: &[String], incoming: &[String]) -> bool {
    if existing.len() != incoming.len() {
        return false;
    }
    let mut a: Vec<Vec<String>> = existing.iter().map(|v| normalize_vevent(v)).collect();
    let mut b: Vec<Vec<String>> = incoming.iter().map(|v| normalize_vevent(v)).collect();
    a.sort();
    b.sort();
    a == b
}

#[derive(Debug)]
enum EventEnd {
    Date(chrono::NaiveDate),
    DateTime(NaiveDateTime),
}

fn parse_ics_value(value: &str, tzid: Option<&str>) -> Option<EventEnd> {
    let trimmed = value.trim();
    let is_utc = trimmed.ends_with('Z');
    let stripped = trimmed.trim_end_matches('Z');
    match stripped.len() {
        8 => chrono::NaiveDate::parse_from_str(stripped, "%Y%m%d")
            .ok()
            .map(EventEnd::Date),
        15 => {
            let naive = NaiveDateTime::parse_from_str(stripped, "%Y%m%dT%H%M%S").ok()?;
            if is_utc || tzid.is_none() {
                Some(EventEnd::DateTime(naive))
            } else {
                match tzid?.parse::<chrono_tz::Tz>() {
                    Ok(tz) => {
                        use chrono::TimeZone;
                        match tz.from_local_datetime(&naive).earliest() {
                            Some(dt) => Some(EventEnd::DateTime(dt.naive_utc())),
                            None => Some(EventEnd::DateTime(naive)),
                        }
                    }
                    Err(_) => Some(EventEnd::DateTime(naive)),
                }
            }
        }
        _ => None,
    }
}

fn event_end_parsed(vevent_text: &str) -> Option<EventEnd> {
    let unfolded = unfold_ics(vevent_text);
    let mut dtend = None;
    let mut dtstart = None;
    for line in unfolded.lines() {
        let trimmed = line.trim();
        let Some(colon_pos) = trimmed.find(':') else {
            continue;
        };
        let params = &trimmed[..colon_pos];
        let prop_name = params.split(';').next().unwrap_or("");
        let tzid = params
            .split(';')
            .skip(1)
            .find_map(|p| p.strip_prefix("TZID="));
        let value = &trimmed[colon_pos + 1..];
        match prop_name {
            "DTEND" => dtend = parse_ics_value(value, tzid),
            "DTSTART" => dtstart = parse_ics_value(value, tzid),
            _ => {}
        }
    }
    dtend.or(dtstart)
}

fn is_event_in_future(vevent_text: &str) -> bool {
    match event_end_parsed(vevent_text) {
        Some(EventEnd::Date(d)) => d > chrono::Local::now().date_naive(),
        Some(EventEnd::DateTime(dt)) => dt > chrono::Utc::now().naive_utc(),
        None => true,
    }
}

struct ExtractedEvents {
    events: HashMap<String, Vec<String>>,
    vtimezones: Vec<String>,
}

fn extract_events(ics_text: &str) -> ExtractedEvents {
    let unfolded = unfold_ics(ics_text);
    let mut events: HashMap<String, Vec<String>> = HashMap::new();
    let mut vtimezones: Vec<String> = Vec::new();
    let mut in_vevent = false;
    let mut in_vtimezone = false;
    let mut current_event = String::new();
    let mut current_uid = String::new();
    let mut current_tz = String::new();

    for line in unfolded.lines() {
        if line.starts_with("BEGIN:VTIMEZONE") {
            in_vtimezone = true;
            current_tz.clear();
        }

        if in_vtimezone {
            current_tz.push_str(line);
            current_tz.push_str("\r\n");
            if line.starts_with("END:VTIMEZONE") {
                in_vtimezone = false;
                vtimezones.push(current_tz.clone());
            }
        } else {
            if line.starts_with("BEGIN:VEVENT") {
                in_vevent = true;
                current_event.clear();
                current_uid.clear();
            }
            if in_vevent {
                current_event.push_str(line);
                current_event.push_str("\r\n");
                if line.starts_with("UID:") {
                    current_uid = line.trim_start_matches("UID:").trim().to_string();
                }
                if line.starts_with("END:VEVENT") {
                    in_vevent = false;
                    if !current_uid.is_empty() {
                        events
                            .entry(current_uid.clone())
                            .or_default()
                            .push(current_event.clone());
                    }
                }
            }
        }
    }
    ExtractedEvents { events, vtimezones }
}

async fn fetch_existing_events(
    client: &Client,
    calendar_base: &str,
) -> Result<HashMap<String, Vec<String>>> {
    let existing_data = sync::fetch_events(client, calendar_base, calendar_base)
        .await
        .context("Failed to fetch existing CalDAV events")?;

    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    for ics_str in &existing_data {
        for (uid, vevents) in extract_events(ics_str).events {
            map.entry(uid).or_default().extend(vevents);
        }
    }
    Ok(map)
}

pub async fn run_reverse_sync(
    ics_url: &str,
    caldav_url: &str,
    calendar_name: &str,
    username: &str,
    password: &str,
    sync_all: bool,
    keep_local: bool,
) -> Result<ReverseSyncStats> {
    let ics_client = Client::new();
    let ics_response = ics_client
        .get(ics_url)
        .send()
        .await
        .context("Failed to fetch ICS file")?;
    let ics_text = ics_response
        .text()
        .await
        .context("Failed to read ICS body")?;

    let extracted = extract_events(&ics_text);

    if extracted.events.is_empty() {
        tracing::warn!("ICS feed at {} returned 0 events, skipping sync", ics_url);
        return Ok(ReverseSyncStats {
            uploaded: 0,
            skipped: 0,
            deleted: 0,
            total: 0,
        });
    }

    let tz_block = extracted.vtimezones.join("");
    let all_remote_uids: HashSet<String> = extracted.events.keys().cloned().collect();
    let events: HashMap<String, Vec<String>> = if sync_all {
        extracted.events
    } else {
        extracted
            .events
            .into_iter()
            .filter(|(_, vevents)| vevents.iter().any(|v| is_event_in_future(v)))
            .collect()
    };

    let auth = format!("{}:{}", username, password);
    let auth_header = format!(
        "Basic {}",
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &auth)
    );

    let mut headers = header::HeaderMap::new();
    headers.insert(
        header::AUTHORIZATION,
        header::HeaderValue::from_str(&auth_header)?,
    );
    let caldav_client = Client::builder().default_headers(headers).build()?;

    let normalized_url = caldav_url.trim_end_matches('/');
    let calendar_base = if normalized_url.ends_with(&format!("/{}", calendar_name)) {
        format!("{}/", normalized_url)
    } else {
        format!("{}/{}/", normalized_url, calendar_name)
    };

    let existing = fetch_existing_events(&caldav_client, &calendar_base).await?;
    tracing::info!(
        "Fetched {} existing events from CalDAV for diff",
        existing.len()
    );

    let mut uploaded = 0;
    let mut skipped = 0;
    let mut errors = 0;

    for (uid, vevent_blocks) in &events {
        if let Some(existing_vevents) = existing.get(uid)
            && events_equal(existing_vevents, vevent_blocks)
        {
            skipped += 1;
            continue;
        }

        let vevent_block = vevent_blocks.join("");
        let wrapped = format!(
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//CalDAV/ICS Sync//EN\r\n{}{}END:VCALENDAR\r\n",
            tz_block, vevent_block
        );

        let event_url = format!("{}{}.ics", calendar_base, uid);

        match caldav_client
            .put(&event_url)
            .header("Content-Type", "text/calendar; charset=utf-8")
            .body(wrapped)
            .send()
            .await
        {
            Ok(res) if res.status().is_success() => {
                uploaded += 1;
            }
            Ok(res) => {
                tracing::warn!("PUT {} returned {}", event_url, res.status());
                errors += 1;
            }
            Err(e) => {
                tracing::error!("PUT {} failed: {}", event_url, e);
                errors += 1;
            }
        }
    }

    if errors > 0 {
        anyhow::bail!("Uploaded {} events but {} failed", uploaded, errors);
    }

    let mut deleted = 0;

    if !keep_local {
        let deletion_candidates: HashSet<String> = if sync_all {
            existing.keys().cloned().collect()
        } else {
            existing
                .iter()
                .filter(|(_, vevents)| vevents.iter().any(|v| is_event_in_future(v)))
                .map(|(uid, _)| uid.clone())
                .collect()
        };

        for uid in deletion_candidates.difference(&all_remote_uids) {
            let event_url = format!("{}{}.ics", calendar_base, uid);
            match caldav_client.delete(&event_url).send().await {
                Ok(res) if res.status().is_success() || res.status().as_u16() == 404 => {
                    deleted += 1;
                    tracing::info!("Deleted orphan event: {}", uid);
                }
                Ok(res) => {
                    tracing::warn!("DELETE {} returned {}", event_url, res.status());
                }
                Err(e) => {
                    tracing::error!("DELETE {} failed: {}", event_url, e);
                }
            }
        }
    }

    Ok(ReverseSyncStats {
        uploaded,
        skipped,
        deleted,
        total: events.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Timelike;

    #[test]
    fn unfold_joins_continuation_lines() {
        let folded = "SUMMARY:Long event\r\n  name here";
        assert!(unfold_ics(folded).contains("SUMMARY:Long event name here"));
    }

    #[test]
    fn normalize_strips_volatile_fields() {
        let vevent = "BEGIN:VEVENT\r\nUID:1\r\nDTSTAMP:20260101T000000Z\r\nSUMMARY:Test\r\nSEQUENCE:3\r\nEND:VEVENT";
        let lines = normalize_vevent(vevent);
        assert!(!lines.iter().any(|l| l.starts_with("DTSTAMP")));
        assert!(!lines.iter().any(|l| l.starts_with("SEQUENCE")));
        assert!(lines.iter().any(|l| l.starts_with("SUMMARY")));
    }

    #[test]
    fn events_equal_ignores_dtstamp_difference() {
        let a = vec![
            "BEGIN:VEVENT\r\nUID:1\r\nDTSTAMP:20260101T000000Z\r\nSUMMARY:Test\r\nEND:VEVENT"
                .to_string(),
        ];
        let b = vec![
            "BEGIN:VEVENT\r\nUID:1\r\nDTSTAMP:20260221T120000Z\r\nSUMMARY:Test\r\nEND:VEVENT"
                .to_string(),
        ];
        assert!(events_equal(&a, &b));
    }

    #[test]
    fn events_not_equal_when_summary_differs() {
        let a = vec!["BEGIN:VEVENT\r\nUID:1\r\nSUMMARY:Meeting A\r\nEND:VEVENT".to_string()];
        let b = vec!["BEGIN:VEVENT\r\nUID:1\r\nSUMMARY:Meeting B\r\nEND:VEVENT".to_string()];
        assert!(!events_equal(&a, &b));
    }

    #[test]
    fn events_equal_different_vevent_count() {
        let a = vec!["BEGIN:VEVENT\r\nUID:1\r\nSUMMARY:Test\r\nEND:VEVENT".to_string()];
        let b = vec![
            "BEGIN:VEVENT\r\nUID:1\r\nSUMMARY:Test\r\nEND:VEVENT".to_string(),
            "BEGIN:VEVENT\r\nUID:1\r\nRECURRENCE-ID:20260308T100000Z\r\nSUMMARY:Override\r\nEND:VEVENT".to_string(),
        ];
        assert!(!events_equal(&a, &b));
    }

    #[test]
    fn extract_events_parses_uids() {
        let ics = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:abc@test\r\nSUMMARY:Test\r\nEND:VEVENT\r\nEND:VCALENDAR";
        let extracted = extract_events(ics);
        assert_eq!(extracted.events.len(), 1);
        assert!(extracted.events.contains_key("abc@test"));
        assert_eq!(extracted.events["abc@test"].len(), 1);
    }

    #[test]
    fn extract_events_groups_recurring_by_uid() {
        let ics = "BEGIN:VCALENDAR\r\n\
            BEGIN:VEVENT\r\n\
            UID:recurring@test\r\n\
            SUMMARY:Weekly Meeting\r\n\
            DTSTART:20260301T100000Z\r\n\
            DTEND:20260301T110000Z\r\n\
            RRULE:FREQ=WEEKLY\r\n\
            END:VEVENT\r\n\
            BEGIN:VEVENT\r\n\
            UID:recurring@test\r\n\
            RECURRENCE-ID:20260308T100000Z\r\n\
            SUMMARY:Weekly Meeting (moved)\r\n\
            DTSTART:20260308T140000Z\r\n\
            DTEND:20260308T150000Z\r\n\
            END:VEVENT\r\n\
            END:VCALENDAR";
        let extracted = extract_events(ics);
        assert_eq!(extracted.events.len(), 1, "both VEVENTs share the same UID");
        assert_eq!(
            extracted.events["recurring@test"].len(),
            2,
            "master + override = 2 VEVENT blocks"
        );
    }

    #[test]
    fn normalize_handles_parameterized_volatile_fields() {
        let vevent = "BEGIN:VEVENT\r\nUID:1\r\nDTSTAMP;VALUE=DATE-TIME:20260101T000000Z\r\nLAST-MODIFIED:20260101T000000Z\r\nSUMMARY:Test\r\nEND:VEVENT";
        let lines = normalize_vevent(vevent);
        assert!(!lines.iter().any(|l| l.starts_with("DTSTAMP")));
        assert!(!lines.iter().any(|l| l.starts_with("LAST-MODIFIED")));
    }

    #[test]
    fn parse_ics_value_date_only() {
        match parse_ics_value("20260301", None) {
            Some(EventEnd::Date(d)) => {
                assert_eq!(d, chrono::NaiveDate::from_ymd_opt(2026, 3, 1).unwrap())
            }
            other => panic!("Expected EventEnd::Date, got {:?}", other),
        }
    }

    #[test]
    fn parse_ics_value_with_time() {
        match parse_ics_value("20260301T100000", None) {
            Some(EventEnd::DateTime(dt)) => assert_eq!(dt.hour(), 10),
            other => panic!("Expected EventEnd::DateTime, got {:?}", other),
        }
    }

    #[test]
    fn parse_ics_value_utc_suffix() {
        match parse_ics_value("20260301T100000Z", None) {
            Some(EventEnd::DateTime(dt)) => assert_eq!(dt.hour(), 10),
            other => panic!("Expected EventEnd::DateTime, got {:?}", other),
        }
    }

    #[test]
    fn parse_ics_value_with_tzid() {
        // March 1 in America/New_York is EST (UTC-5), so 10:00 local = 15:00 UTC
        match parse_ics_value("20260301T100000", Some("America/New_York")) {
            Some(EventEnd::DateTime(dt)) => assert_eq!(dt.hour(), 15),
            other => panic!(
                "Expected EventEnd::DateTime with UTC hour 15, got {:?}",
                other
            ),
        }
    }

    #[test]
    fn parse_ics_value_with_unrecognized_tzid() {
        match parse_ics_value("20260301T100000", Some("Fake/Timezone")) {
            Some(EventEnd::DateTime(dt)) => assert_eq!(dt.hour(), 10),
            other => panic!("Expected EventEnd::DateTime with hour 10, got {:?}", other),
        }
    }

    #[test]
    fn event_end_parsed_uses_dtend() {
        let vevent =
            "BEGIN:VEVENT\r\nDTSTART:20260101T090000Z\r\nDTEND:20260101T100000Z\r\nEND:VEVENT";
        match event_end_parsed(vevent) {
            Some(EventEnd::DateTime(dt)) => assert_eq!(dt.hour(), 10),
            other => panic!("Expected EventEnd::DateTime, got {:?}", other),
        }
    }

    #[test]
    fn event_end_parsed_falls_back_to_dtstart() {
        let vevent = "BEGIN:VEVENT\r\nDTSTART:20260101T090000Z\r\nEND:VEVENT";
        match event_end_parsed(vevent) {
            Some(EventEnd::DateTime(dt)) => assert_eq!(dt.hour(), 9),
            other => panic!("Expected EventEnd::DateTime, got {:?}", other),
        }
    }

    #[test]
    fn event_end_parsed_handles_tzid() {
        // March 1 in America/New_York is EST (UTC-5), so 10:00 local = 15:00 UTC
        let vevent = "BEGIN:VEVENT\r\nDTEND;TZID=America/New_York:20260301T100000\r\nEND:VEVENT";
        match event_end_parsed(vevent) {
            Some(EventEnd::DateTime(dt)) => assert_eq!(dt.hour(), 15),
            other => panic!("Expected EventEnd::DateTime, got {:?}", other),
        }
    }

    #[test]
    fn is_event_in_future_past_event() {
        let vevent = "BEGIN:VEVENT\r\nDTEND:20200101T100000Z\r\nEND:VEVENT";
        assert!(!is_event_in_future(vevent));
    }

    #[test]
    fn is_event_in_future_future_event() {
        let vevent = "BEGIN:VEVENT\r\nDTEND:20990101T100000Z\r\nEND:VEVENT";
        assert!(is_event_in_future(vevent));
    }

    #[test]
    fn is_event_in_future_unparseable_defaults_true() {
        let vevent = "BEGIN:VEVENT\r\nSUMMARY:No dates\r\nEND:VEVENT";
        assert!(is_event_in_future(vevent));
    }

    #[test]
    fn parse_ics_value_dst_gap_falls_back_to_naive() {
        // 2:30 AM on March 8, 2026 falls in the DST gap for America/New_York
        // (clocks spring forward from 2:00 to 3:00)
        match parse_ics_value("20260308T023000", Some("America/New_York")) {
            Some(EventEnd::DateTime(dt)) => {
                assert_eq!(dt.hour(), 2);
                assert_eq!(dt.minute(), 30);
            }
            other => panic!("Expected EventEnd::DateTime fallback, got {:?}", other),
        }
    }

    #[test]
    fn extract_events_captures_vtimezone_blocks() {
        let ics = "BEGIN:VCALENDAR\r\n\
            VERSION:2.0\r\n\
            BEGIN:VTIMEZONE\r\n\
            TZID:America/New_York\r\n\
            BEGIN:STANDARD\r\n\
            DTSTART:19701101T020000\r\n\
            RRULE:FREQ=YEARLY;BYMONTH=11;BYDAY=1SU\r\n\
            TZOFFSETFROM:-0400\r\n\
            TZOFFSETTO:-0500\r\n\
            END:STANDARD\r\n\
            END:VTIMEZONE\r\n\
            BEGIN:VEVENT\r\n\
            UID:tz-test@example\r\n\
            DTSTART;TZID=America/New_York:20260301T100000\r\n\
            SUMMARY:TZ Test\r\n\
            END:VEVENT\r\n\
            END:VCALENDAR";
        let extracted = extract_events(ics);
        assert_eq!(extracted.events.len(), 1);
        assert!(extracted.events.contains_key("tz-test@example"));
        assert_eq!(extracted.vtimezones.len(), 1);
        assert!(extracted.vtimezones[0].contains("TZID:America/New_York"));
        assert!(extracted.vtimezones[0].starts_with("BEGIN:VTIMEZONE"));
        assert!(extracted.vtimezones[0].contains("END:VTIMEZONE"));
    }
}
