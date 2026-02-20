use anyhow::{Context, Result};
use reqwest::{Client, header};

fn toggle_slash(url: &str) -> String {
    if url.ends_with('/') {
        url.trim_end_matches('/').to_string()
    } else {
        format!("{}/", url)
    }
}

async fn propfind(client: &Client, url: &str, body: &str) -> Result<reqwest::Response> {
    client
        .request(reqwest::Method::from_bytes(b"PROPFIND").unwrap(), url)
        .header("Depth", "1")
        .header(header::CONTENT_TYPE, "application/xml; charset=utf-8")
        .body(body.to_string())
        .send()
        .await?
        .error_for_status()
        .map_err(Into::into)
}

pub async fn fetch_calendars(client: &Client, url: &str) -> Result<Vec<String>> {
    let propfind_body = r#"<?xml version="1.0" encoding="utf-8" ?>
<d:propfind xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
  <d:prop>
     <d:resourcetype />
     <d:displayname />
     <c:supported-calendar-component-set />
  </d:prop>
</d:propfind>"#;

    let res = match propfind(client, url, propfind_body).await {
        Ok(r) => r,
        Err(_) => {
            let alt = toggle_slash(url);
            tracing::info!("Retrying PROPFIND with toggled slash: {}", alt);
            propfind(client, &alt, propfind_body).await?
        }
    };

    let text = res.text().await?;
    let doc = roxmltree::Document::parse(&text)?;

    let mut calendar_urls = Vec::new();
    for node in doc.descendants() {
        if node.has_tag_name(("DAV:", "response")) {
            let mut is_calendar = false;
            let mut href = None;

            for child in node.children() {
                if child.has_tag_name(("DAV:", "href")) {
                    href = child.text();
                } else if child.has_tag_name(("DAV:", "propstat")) {
                    for propstat_child in child.children() {
                        if propstat_child.has_tag_name(("DAV:", "prop")) {
                            for prop in propstat_child.children() {
                                if prop.has_tag_name(("DAV:", "resourcetype")) {
                                    for rt_child in prop.children() {
                                        if rt_child.has_tag_name((
                                            "urn:ietf:params:xml:ns:caldav",
                                            "calendar",
                                        )) {
                                            is_calendar = true;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            if is_calendar && let Some(h) = href {
                calendar_urls.push(h.to_string());
            }
        }
    }

    Ok(calendar_urls)
}

pub async fn fetch_events(
    client: &Client,
    base_url: &str,
    calendar_path: &str,
) -> Result<Vec<String>> {
    let url = if calendar_path.starts_with("http") {
        calendar_path.to_string()
    } else {
        let parsed = reqwest::Url::parse(base_url)?;
        format!(
            "{}://{}{}",
            parsed.scheme(),
            parsed.host_str().unwrap_or(""),
            calendar_path
        )
    };

    let report_body = r#"<?xml version="1.0" encoding="utf-8" ?>
<c:calendar-query xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
  <d:prop>
    <d:getetag />
    <c:calendar-data />
  </d:prop>
  <c:filter>
    <c:comp-filter name="VCALENDAR">
      <c:comp-filter name="VEVENT" />
    </c:comp-filter>
  </c:filter>
</c:calendar-query>"#;

    let res = client
        .request(reqwest::Method::from_bytes(b"REPORT").unwrap(), &url)
        .header("Depth", "1")
        .header(header::CONTENT_TYPE, "application/xml; charset=utf-8")
        .body(report_body)
        .send()
        .await?;

    let text = res.text().await?;
    let doc = roxmltree::Document::parse(&text)?;

    let mut ics_events = Vec::new();
    for node in doc.descendants() {
        if node.has_tag_name(("urn:ietf:params:xml:ns:caldav", "calendar-data"))
            && let Some(data) = node.text()
        {
            ics_events.push(data.to_string());
        }
    }

    Ok(ics_events)
}

pub async fn run_sync(
    caldav_url: &str,
    username: &str,
    password: &str,
) -> Result<(usize, usize, String)> {
    let mut headers = header::HeaderMap::new();
    let auth = format!("{}:{}", username, password);
    let auth_header = format!(
        "Basic {}",
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &auth)
    );
    headers.insert(
        header::AUTHORIZATION,
        header::HeaderValue::from_str(&auth_header)?,
    );

    let client = Client::builder().default_headers(headers).build()?;

    let calendar_paths = fetch_calendars(&client, caldav_url)
        .await
        .context("Failed to fetch calendars")?;
    let calendar_count = calendar_paths.len();

    let mut combined_events = Vec::new();
    let mut event_count = 0;

    for path in &calendar_paths {
        if let Ok(events_data) = fetch_events(&client, caldav_url, path).await {
            for ics_str in events_data {
                let mut in_vevent = false;
                let mut current_event = String::new();
                for line in ics_str.lines() {
                    if line.starts_with("BEGIN:VEVENT") {
                        in_vevent = true;
                    }
                    if in_vevent {
                        current_event.push_str(line);
                        current_event.push_str("\r\n");
                    }
                    if line.starts_with("END:VEVENT") {
                        in_vevent = false;
                        combined_events.push(current_event.clone());
                        current_event.clear();
                        event_count += 1;
                    }
                }
            }
        }
    }

    let mut output = String::new();
    output.push_str(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//CalDAV/ICS Sync//EN\r\nCALSCALE:GREGORIAN\r\nMETHOD:PUBLISH\r\n",
    );
    for ev in combined_events {
        output.push_str(&ev);
    }
    output.push_str("END:VCALENDAR\r\n");

    Ok((event_count, calendar_count, output))
}
