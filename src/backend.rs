use anyhow::Result;
use async_trait::async_trait;
use chrono::{NaiveDate, NaiveDateTime};

use crate::models::CalendarEvent;

#[async_trait]
pub trait CalendarBackend: Send {
    async fn list_events_range(
        &mut self,
        start: NaiveDate,
        end: NaiveDate,
    ) -> Result<Vec<CalendarEvent>>;
    async fn create_event(
        &mut self,
        summary: &str,
        description: Option<&str>,
        start: NaiveDateTime,
        end: NaiveDateTime,
    ) -> Result<CalendarEvent>;
    async fn update_event(
        &mut self,
        event_id: &str,
        summary: &str,
        description: Option<&str>,
        start: NaiveDateTime,
        end: NaiveDateTime,
    ) -> Result<()>;
    async fn delete_event(&mut self, event_id: &str) -> Result<()>;
}
