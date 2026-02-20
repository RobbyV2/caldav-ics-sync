use anyhow::{Context, Result};
use reqwest::{Client, header};

pub async fn run_reverse_sync(
    ics_url: &str,
    caldav_url: &str,
    calendar_name: &str,
    username: &str,
    password: &str,
    _sync_all: bool,
    _keep_local: bool,
) -> Result<(usize, usize)> {
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

    let mut events: Vec<(String, String)> = Vec::new();
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

    let calendar_base = if caldav_url.ends_with('/') {
        format!("{}{}/", caldav_url, calendar_name)
    } else {
        format!("{}/{}/", caldav_url, calendar_name)
    };

    let mut uploaded = 0;
    let mut errors = 0;

    for (uid, vevent_data) in &events {
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

    Ok((uploaded, events.len()))
}
