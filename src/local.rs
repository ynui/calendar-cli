use std::path::PathBuf;

use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::{NaiveDate, NaiveDateTime};
use serde::{Deserialize, Serialize};

use crate::backend::CalendarBackend;
use crate::models::CalendarEvent;

#[derive(Debug, Serialize, Deserialize)]
struct LocalStore {
    events: Vec<LocalEvent>,
    next_id: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct LocalEvent {
    id: u64,
    summary: String,
    description: Option<String>,
    start: Option<NaiveDateTime>,
    end: Option<NaiveDateTime>,
}

pub struct LocalCalendar {
    path: PathBuf,
    store: LocalStore,
}

impl LocalCalendar {
    pub fn new(path: PathBuf) -> Self {
        let store = if path.exists() {
            std::fs::read_to_string(&path)
                .ok()
                .and_then(|raw| serde_json::from_str(&raw).ok())
                .unwrap_or(LocalStore {
                    events: Vec::new(),
                    next_id: 1,
                })
        } else {
            LocalStore {
                events: Vec::new(),
                next_id: 1,
            }
        };
        Self { path, store }
    }

    fn save(&self) -> Result<()> {
        let json = serde_json::to_string_pretty(&self.store)?;
        std::fs::write(&self.path, json).context("Failed to save local events")
    }
}

#[async_trait]
impl CalendarBackend for LocalCalendar {
    async fn list_events_range(
        &mut self,
        start: NaiveDate,
        end: NaiveDate,
    ) -> Result<Vec<CalendarEvent>> {
        let events: Vec<CalendarEvent> = self
            .store
            .events
            .iter()
            .filter(|e| match (e.start, e.end) {
                (Some(s), Some(e)) => s.date() <= end && e.date() >= start,
                (Some(s), None) => s.date() >= start && s.date() <= end,
                (None, _) => false,
            })
            .map(|e| CalendarEvent {
                id: format!("local-{}", e.id),
                summary: e.summary.clone(),
                description: e.description.clone(),
                start: e.start,
                end: e.end,
            })
            .collect();
        Ok(events)
    }

    async fn create_event(
        &mut self,
        summary: &str,
        description: Option<&str>,
        start: NaiveDateTime,
        end: NaiveDateTime,
    ) -> Result<CalendarEvent> {
        let id = self.store.next_id;
        self.store.next_id += 1;

        let event = LocalEvent {
            id,
            summary: summary.to_string(),
            description: description.map(String::from),
            start: Some(start),
            end: Some(end),
        };
        self.store.events.push(event.clone());
        self.save()?;

        Ok(CalendarEvent {
            id: format!("local-{}", id),
            summary: event.summary,
            description: event.description,
            start: event.start,
            end: event.end,
        })
    }

    async fn update_event(
        &mut self,
        event_id: &str,
        summary: &str,
        description: Option<&str>,
        start: NaiveDateTime,
        end: NaiveDateTime,
    ) -> Result<()> {
        let local_id = event_id
            .strip_prefix("local-")
            .and_then(|s| s.parse::<u64>().ok())
            .context("Invalid local event ID")?;

        let ev = self
            .store
            .events
            .iter_mut()
            .find(|e| e.id == local_id)
            .context("Event not found")?;

        ev.summary = summary.to_string();
        ev.description = description.map(String::from);
        ev.start = Some(start);
        ev.end = Some(end);
        self.save()
    }

    async fn delete_event(&mut self, event_id: &str) -> Result<()> {
        let local_id = event_id
            .strip_prefix("local-")
            .and_then(|s| s.parse::<u64>().ok())
            .context("Invalid local event ID")?;

        let len_before = self.store.events.len();
        self.store.events.retain(|e| e.id != local_id);
        if self.store.events.len() == len_before {
            anyhow::bail!("Event not found");
        }
        self.save()
    }
}
