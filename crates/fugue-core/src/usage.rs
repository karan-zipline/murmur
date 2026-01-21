use serde::Deserialize;
use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime};

pub const BILLING_WINDOW_DURATION: Duration = Duration::hours(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub output_tokens: i64,
}

pub fn default_pro_limits() -> Limits {
    Limits {
        output_tokens: 1_000_000,
    }
}

pub fn default_max_limits() -> Limits {
    Limits {
        output_tokens: 2_000_000,
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Usage {
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_creation_tokens: i64,
    pub cache_read_tokens: i64,
    pub message_count: u32,
    pub first_message_at: Option<OffsetDateTime>,
    pub last_message_at: Option<OffsetDateTime>,
}

impl Usage {
    pub fn add(&mut self, other: Usage) {
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.cache_creation_tokens += other.cache_creation_tokens;
        self.cache_read_tokens += other.cache_read_tokens;
        self.message_count += other.message_count;

        match (self.first_message_at, other.first_message_at) {
            (None, v) => self.first_message_at = v,
            (Some(a), Some(b)) if b < a => self.first_message_at = Some(b),
            _ => {}
        }

        match (self.last_message_at, other.last_message_at) {
            (None, v) => self.last_message_at = v,
            (Some(a), Some(b)) if b > a => self.last_message_at = Some(b),
            _ => {}
        }
    }

    pub fn total_input_tokens(&self) -> i64 {
        self.input_tokens + self.cache_creation_tokens + self.cache_read_tokens
    }

    pub fn total_tokens(&self) -> i64 {
        self.total_input_tokens() + self.output_tokens
    }

    pub fn percent_int(&self, limits: Limits) -> i32 {
        if limits.output_tokens <= 0 {
            return 0;
        }
        ((self.output_tokens.saturating_mul(100)) / limits.output_tokens) as i32
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BillingWindow {
    pub start: OffsetDateTime,
    pub end: OffsetDateTime,
}

impl BillingWindow {
    pub fn time_remaining(&self, now: OffsetDateTime) -> Duration {
        let remaining = self.end - now;
        if remaining.is_negative() {
            Duration::ZERO
        } else {
            remaining
        }
    }

    pub fn contains(&self, ts: OffsetDateTime) -> bool {
        ts >= self.start && ts < self.end
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageEntry {
    pub timestamp: OffsetDateTime,
    pub usage: Usage,
}

#[derive(Debug, Deserialize)]
struct JsonlEntry {
    #[serde(default)]
    r#type: String,
    #[serde(default)]
    timestamp: String,
    #[serde(default)]
    message: Option<JsonlMessage>,
}

#[derive(Debug, Deserialize)]
struct JsonlMessage {
    #[serde(default)]
    usage: Option<JsonlTokenUsage>,
}

#[derive(Debug, Deserialize)]
struct JsonlTokenUsage {
    #[serde(default)]
    input_tokens: i64,
    #[serde(default)]
    output_tokens: i64,
    #[serde(default)]
    cache_creation_input_tokens: i64,
    #[serde(default)]
    cache_read_input_tokens: i64,
}

pub fn parse_usage_entry(line: &str) -> Option<UsageEntry> {
    let entry: JsonlEntry = serde_json::from_str(line).ok()?;
    if entry.r#type != "assistant" {
        return None;
    }

    let msg = entry.message?;
    let tokens = msg.usage?;
    let timestamp = OffsetDateTime::parse(&entry.timestamp, &Rfc3339).ok()?;

    Some(UsageEntry {
        timestamp,
        usage: Usage {
            input_tokens: tokens.input_tokens,
            output_tokens: tokens.output_tokens,
            cache_creation_tokens: tokens.cache_creation_input_tokens,
            cache_read_tokens: tokens.cache_read_input_tokens,
            message_count: 1,
            first_message_at: Some(timestamp),
            last_message_at: Some(timestamp),
        },
    })
}

pub fn floor_to_hour(t: OffsetDateTime) -> OffsetDateTime {
    t.replace_minute(0)
        .and_then(|t| t.replace_second(0))
        .and_then(|t| t.replace_nanosecond(0))
        .unwrap_or(t)
}

pub fn active_billing_windows(
    now: OffsetDateTime,
    mut timestamps: Vec<OffsetDateTime>,
) -> Vec<BillingWindow> {
    if timestamps.is_empty() {
        return vec![];
    }

    timestamps.sort();

    let mut blocks = Vec::new();
    let mut current: Option<BillingWindow> = None;
    let mut last_ts: Option<OffsetDateTime> = None;

    for ts in timestamps {
        match (current, last_ts) {
            (None, _) => {
                let start = floor_to_hour(ts);
                current = Some(BillingWindow {
                    start,
                    end: start + BILLING_WINDOW_DURATION,
                });
                last_ts = Some(ts);
            }
            (Some(block), Some(prev)) => {
                let time_since_block_start = ts - block.start;
                let time_since_last_entry = ts - prev;

                if time_since_block_start > BILLING_WINDOW_DURATION
                    || time_since_last_entry > BILLING_WINDOW_DURATION
                {
                    blocks.push(block);
                    let start = floor_to_hour(ts);
                    current = Some(BillingWindow {
                        start,
                        end: start + BILLING_WINDOW_DURATION,
                    });
                } else {
                    current = Some(block);
                }

                last_ts = Some(ts);
            }
            (Some(block), None) => {
                current = Some(block);
                last_ts = Some(ts);
            }
        }
    }

    if let Some(block) = current {
        blocks.push(block);
    }

    let cutoff = now - Duration::hours(1);
    blocks.into_iter().filter(|b| b.end > cutoff).collect()
}

pub fn current_billing_window(
    now: OffsetDateTime,
    timestamps: Vec<OffsetDateTime>,
) -> BillingWindow {
    let active = active_billing_windows(now, timestamps);
    if let Some(window) = active.last() {
        return *window;
    }

    let start = floor_to_hour(now - BILLING_WINDOW_DURATION);
    BillingWindow {
        start,
        end: start + BILLING_WINDOW_DURATION,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_assistant_entry_with_usage() {
        let line = r#"{"type":"assistant","timestamp":"2026-01-03T10:00:00Z","message":{"usage":{"input_tokens":10,"output_tokens":20,"cache_creation_input_tokens":3,"cache_read_input_tokens":4}}}"#;
        let got = parse_usage_entry(line).expect("expected usage entry");
        assert_eq!(got.usage.input_tokens, 10);
        assert_eq!(got.usage.output_tokens, 20);
        assert_eq!(got.usage.cache_creation_tokens, 3);
        assert_eq!(got.usage.cache_read_tokens, 4);
        assert_eq!(got.usage.message_count, 1);
        assert_eq!(
            got.timestamp,
            OffsetDateTime::parse("2026-01-03T10:00:00Z", &Rfc3339).unwrap()
        );
    }

    #[test]
    fn ignores_non_assistant_entries() {
        let line = r#"{"type":"user","timestamp":"2026-01-03T10:00:00Z","message":{"usage":{"output_tokens":1}}}"#;
        assert!(parse_usage_entry(line).is_none());
    }

    #[test]
    fn floor_to_hour_clears_sub_hour_components() {
        let ts = OffsetDateTime::parse("2026-01-03T14:30:45Z", &Rfc3339).unwrap();
        let got = floor_to_hour(ts);
        assert_eq!(
            got,
            OffsetDateTime::parse("2026-01-03T14:00:00Z", &Rfc3339).unwrap()
        );
    }

    #[test]
    fn percent_int_matches_fab_style_truncation() {
        let usage = Usage {
            output_tokens: 335_000,
            ..Default::default()
        };
        assert_eq!(
            usage.percent_int(Limits {
                output_tokens: 500_000
            }),
            67
        );
    }

    #[test]
    fn current_billing_window_prefers_most_recent_active_block() {
        let now = OffsetDateTime::parse("2026-01-03T20:30:00Z", &Rfc3339).unwrap();
        let timestamps = vec![
            OffsetDateTime::parse("2026-01-03T10:10:00Z", &Rfc3339).unwrap(),
            OffsetDateTime::parse("2026-01-03T10:20:00Z", &Rfc3339).unwrap(),
            OffsetDateTime::parse("2026-01-03T20:10:00Z", &Rfc3339).unwrap(),
        ];

        let window = current_billing_window(now, timestamps);
        assert_eq!(
            window.start,
            OffsetDateTime::parse("2026-01-03T20:00:00Z", &Rfc3339).unwrap()
        );
        assert_eq!(
            window.end,
            OffsetDateTime::parse("2026-01-04T01:00:00Z", &Rfc3339).unwrap()
        );
    }
}
