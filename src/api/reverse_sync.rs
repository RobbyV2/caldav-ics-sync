use std::collections::HashMap;

use anyhow::{Context, Result};
use reqwest::{Client, header};

use crate::api::sync;

const VOLATILE_FIELDS: &[&str] = &["DTSTAMP", "SEQUENCE", "LAST-MODIFIED", "CREATED"];

fn unfold_ics(text: &str) -> String {
    let mut lines = Vec::new();
    for line in text.lines() {
        if (line.starts_with(' ') || line.starts_with('\t')) && !lines.is_empty() {
            let last: &mut String = lines.last_mut().unwrap();
            last.push_str(&line[1..]);
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

fn events_equal(existing: &str, incoming: &str) -> bool {
    normalize_vevent(existing) == normalize_vevent(incoming)
}

fn extract_events(ics_text: &str) -> Vec<(String, String)> {
    let mut events = Vec::new();
    let mut in_vevent = false;
    let mut current_event = String::new();
    let mut current_uid = String::new();

    for line in ics_text.lines() {
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
        }
        if line.starts_with("END:VEVENT") {
            in_vevent = false;
            if !current_uid.is_empty() {
                events.push((current_uid.clone(), current_event.clone()));
            }
        }
    }
    events
}

async fn fetch_existing_events(client: &Client, calendar_base: &str) -> HashMap<String, String> {
    let existing_data = sync::fetch_events(client, calendar_base, calendar_base)
        .await
        .unwrap_or_default();

    let mut map = HashMap::new();
    for ics_str in &existing_data {
        for (uid, vevent) in extract_events(ics_str) {
            map.insert(uid, vevent);
        }
    }
    map
}

pub async fn run_reverse_sync(
    ics_url: &str,
    caldav_url: &str,
    calendar_name: &str,
    username: &str,
    password: &str,
    _sync_all: bool,
    _keep_local: bool,
) -> Result<(usize, usize, usize)> {
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

    let events = extract_events(&ics_text);

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
    let calendar_base = if normalized_url.ends_with(calendar_name) {
        format!("{}/", normalized_url)
    } else {
        format!("{}/{}/", normalized_url, calendar_name)
    };

    let existing = fetch_existing_events(&caldav_client, &calendar_base).await;
    tracing::info!(
        "Fetched {} existing events from CalDAV for diff",
        existing.len()
    );

    let mut uploaded = 0;
    let mut skipped = 0;
    let mut errors = 0;

    for (uid, vevent_data) in &events {
        if let Some(existing_vevent) = existing.get(uid)
            && events_equal(existing_vevent, vevent_data)
        {
            skipped += 1;
            continue;
        }

        let wrapped = format!(
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//CalDAV/ICS Sync//EN\r\n{}\r\nEND:VCALENDAR\r\n",
            vevent_data
        );

        let event_url = format!("{}{}.ics", calendar_base, uid);

        match caldav_client
            .put(&event_url)
            .header("Content-Type", "text/calendar; charset=utf-8")
            .body(wrapped)
            .send()
            .await
        {
            Ok(res)
                if res.status().is_success()
                    || res.status().as_u16() == 201
                    || res.status().as_u16() == 204 =>
            {
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

    Ok((uploaded, skipped, events.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unfold_joins_continuation_lines() {
        // Two spaces: first is the RFC 5545 fold marker (stripped), second is content
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
        let a = "BEGIN:VEVENT\r\nUID:1\r\nDTSTAMP:20260101T000000Z\r\nSUMMARY:Test\r\nEND:VEVENT";
        let b = "BEGIN:VEVENT\r\nUID:1\r\nDTSTAMP:20260221T120000Z\r\nSUMMARY:Test\r\nEND:VEVENT";
        assert!(events_equal(a, b));
    }

    #[test]
    fn events_not_equal_when_summary_differs() {
        let a = "BEGIN:VEVENT\r\nUID:1\r\nSUMMARY:Meeting A\r\nEND:VEVENT";
        let b = "BEGIN:VEVENT\r\nUID:1\r\nSUMMARY:Meeting B\r\nEND:VEVENT";
        assert!(!events_equal(a, b));
    }

    #[test]
    fn extract_events_parses_uids() {
        let ics = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:abc@test\r\nSUMMARY:Test\r\nEND:VEVENT\r\nEND:VCALENDAR";
        let events = extract_events(ics);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "abc@test");
    }

    #[test]
    fn normalize_handles_parameterized_volatile_fields() {
        let vevent = "BEGIN:VEVENT\r\nUID:1\r\nDTSTAMP;VALUE=DATE-TIME:20260101T000000Z\r\nLAST-MODIFIED:20260101T000000Z\r\nSUMMARY:Test\r\nEND:VEVENT";
        let lines = normalize_vevent(vevent);
        assert!(!lines.iter().any(|l| l.starts_with("DTSTAMP")));
        assert!(!lines.iter().any(|l| l.starts_with("LAST-MODIFIED")));
    }
}
