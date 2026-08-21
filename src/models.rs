use chrono::{NaiveDate, NaiveDateTime};

#[derive(Debug, Clone)]
pub struct CalendarEvent {
    pub id: String,
    pub summary: String,
    pub description: Option<String>,
    pub start: Option<NaiveDateTime>,
    pub end: Option<NaiveDateTime>,
}

pub struct FormState {
    pub fields: Vec<FormField>,
    pub focus: usize,
}

pub struct FormField {
    pub label: &'static str,
    pub value: String,
    pub cursor: usize,
}

impl FormState {
    pub fn new(date: NaiveDate) -> Self {
        Self {
            fields: vec![
                FormField {
                    label: "Title",
                    value: String::new(),
                    cursor: 0,
                },
                FormField {
                    label: "Date",
                    value: date.format("%Y-%m-%d").to_string(),
                    cursor: 0,
                },
                FormField {
                    label: "Start",
                    value: "09:00".into(),
                    cursor: 0,
                },
                FormField {
                    label: "End",
                    value: "10:00".into(),
                    cursor: 0,
                },
            ],
            focus: 0,
        }
    }

    pub fn from_event(event: &CalendarEvent) -> Self {
        let date = event
            .start
            .map(|d| d.format("%Y-%m-%d").to_string())
            .unwrap_or_default();
        let start = event
            .start
            .map(|d| d.format("%H:%M").to_string())
            .unwrap_or_default();
        let end = event
            .end
            .map(|d| d.format("%H:%M").to_string())
            .unwrap_or_default();
        let desc = event.description.clone().unwrap_or_default();
        Self {
            fields: vec![
                FormField {
                    label: "Title",
                    value: event.summary.clone(),
                    cursor: 0,
                },
                FormField {
                    label: "Date",
                    value: date,
                    cursor: 0,
                },
                FormField {
                    label: "Start",
                    value: start,
                    cursor: 0,
                },
                FormField {
                    label: "End",
                    value: end,
                    cursor: 0,
                },
                FormField {
                    label: "Description",
                    value: desc,
                    cursor: 0,
                },
            ],
            focus: 0,
        }
    }
}
