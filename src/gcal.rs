use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, TimeZone, Utc};
use serde::Deserialize;

use crate::auth::GoogleAuth;
use crate::backend::CalendarBackend;
use crate::models::CalendarEvent;

// ── Google Calendar API response types ──────────────────────────

#[derive(Debug, Deserialize)]
struct EventListResponse {
    #[serde(default)]
    items: Option<Vec<RawEvent>>,
}

#[derive(Debug, Deserialize)]
struct RawEvent {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    start: Option<RawDateTime>,
    #[serde(default)]
    end: Option<RawDateTime>,
}

#[derive(Debug, Deserialize)]
struct RawDateTime {
    #[serde(default, rename = "dateTime")]
    date_time: Option<chrono::DateTime<Utc>>,
    #[serde(default, rename = "date")]
    date: Option<chrono::NaiveDate>,
}

const API_BASE: &str = "https://www.googleapis.com/calendar/v3/calendars/primary/events";

// ── GoogleCalendar backend ──────────────────────────────────────

pub struct GoogleCalendar {
    auth: GoogleAuth,
    http: reqwest::Client,
}

impl GoogleCalendar {
    pub fn new(auth: GoogleAuth) -> Self {
        Self {
            auth,
            http: reqwest::Client::new(),
        }
    }

    pub fn needs_auth(&self) -> bool {
        self.auth.needs_auth()
    }

    pub async fn authenticate(&mut self) -> Result<()> {
        self.auth.authenticate().await
    }
}

#[async_trait]
impl CalendarBackend for GoogleCalendar {
    async fn list_events_range(
        &mut self,
        start: NaiveDate,
        end: NaiveDate,
    ) -> Result<Vec<CalendarEvent>> {
        let token = self.auth.get_access_token().await?;

        let time_min = start
            .and_hms_opt(0, 0, 0)
            .expect("valid time")
            .and_utc()
            .to_rfc3339();
        let time_max = end
            .and_hms_opt(23, 59, 59)
            .expect("valid time")
            .and_utc()
            .to_rfc3339();

        let resp = self
            .http
            .get(API_BASE)
            .bearer_auth(&token)
            .query(&[
                ("timeMin", time_min.as_str()),
                ("timeMax", time_max.as_str()),
                ("singleEvents", "true"),
                ("orderBy", "startTime"),
            ])
            .send()
            .await
            .context("Failed to list events")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await?;
            anyhow::bail!("List events API error {}: {}", status, text);
        }

        let list: EventListResponse = resp.json().await?;
        let events = list.items.unwrap_or_default();
        Ok(events.into_iter().filter_map(raw_to_model).collect())
    }

    async fn create_event(
        &mut self,
        summary: &str,
        description: Option<&str>,
        start: NaiveDateTime,
        end: NaiveDateTime,
    ) -> Result<CalendarEvent> {
        let token = self.auth.get_access_token().await?;

        let local_to_utc = |ndt: NaiveDateTime| -> DateTime<Utc> {
            Local
                .from_local_datetime(&ndt)
                .earliest()
                .expect("invalid local time due to DST transition")
                .to_utc()
        };

        let mut body = serde_json::json!({
            "summary": summary,
            "start": {
                "dateTime": local_to_utc(start).to_rfc3339(),
                "timeZone": "UTC",
            },
            "end": {
                "dateTime": local_to_utc(end).to_rfc3339(),
                "timeZone": "UTC",
            },
        });
        if let Some(desc) = description {
            body["description"] = serde_json::json!(desc);
        }

        let resp = self
            .http
            .post(API_BASE)
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await
            .context("Failed to create event")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await?;
            anyhow::bail!("Create event API error {}: {}", status, text);
        }

        let raw: RawEvent = resp.json().await?;
        raw_to_model(raw).context("Failed to parse created event")
    }

    async fn update_event(
        &mut self,
        event_id: &str,
        summary: &str,
        description: Option<&str>,
        start: NaiveDateTime,
        end: NaiveDateTime,
    ) -> Result<()> {
        let token = self.auth.get_access_token().await?;

        let local_to_utc = |ndt: NaiveDateTime| -> DateTime<Utc> {
            Local
                .from_local_datetime(&ndt)
                .earliest()
                .expect("invalid local time due to DST transition")
                .to_utc()
        };

        let mut body = serde_json::json!({
            "summary": summary,
            "start": {
                "dateTime": local_to_utc(start).to_rfc3339(),
                "timeZone": "UTC",
            },
            "end": {
                "dateTime": local_to_utc(end).to_rfc3339(),
                "timeZone": "UTC",
            },
        });
        if let Some(desc) = description {
            body["description"] = serde_json::json!(desc);
        }

        let url = format!("{}/{}", API_BASE, event_id);
        let resp = self
            .http
            .put(&url)
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await
            .context("Failed to update event")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await?;
            anyhow::bail!("Update event API error {}: {}", status, text);
        }
        Ok(())
    }

    async fn delete_event(&mut self, event_id: &str) -> Result<()> {
        let token = self.auth.get_access_token().await?;

        let url = format!("{}/{}", API_BASE, event_id);
        let resp = self
            .http
            .delete(&url)
            .bearer_auth(&token)
            .send()
            .await
            .context("Failed to delete event")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await?;
            anyhow::bail!("Delete event API error {}: {}", status, text);
        }
        Ok(())
    }
}

// ── Conversion helpers ──────────────────────────────────────────

fn raw_to_model(raw: RawEvent) -> Option<CalendarEvent> {
    let rs = raw.start.as_ref()?;
    let re = raw.end.as_ref()?;
    let start = rs
        .date_time
        .map(|dt| dt.with_timezone(&Local).naive_local())
        .or_else(|| rs.date.map(|d| d.and_hms_opt(0, 0, 0).unwrap()));
    let end = re
        .date_time
        .map(|dt| dt.with_timezone(&Local).naive_local())
        .or_else(|| re.date.map(|d| d.and_hms_opt(23, 59, 59).unwrap()));
    Some(CalendarEvent {
        id: raw.id?,
        summary: raw.summary?,
        description: raw.description,
        start,
        end,
    })
}
