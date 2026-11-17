use jiff::{Span, Zoned};
use std::time::Duration;

#[derive(Debug, Clone)]
pub enum CommitTimeBound {
    Fixed(Zoned),
    Relative(Span),
}

impl CommitTimeBound {
    pub fn parse(value: &str) -> anyhow::Result<Self> {
        if let Ok(zoned) = value.parse::<Zoned>() {
            Ok(Self::Fixed(zoned))
        } else if let Ok(relative) = value.parse::<Span>() {
            Ok(Self::Relative(relative))
        } else {
            anyhow::bail!(
                "expected a date or datetime with a bracketed time zone (e.g. '2026-11-13T00:00:00[UTC]') or a relative span (e.g. '4 days ago')"
            )
        }
    }

    pub fn resolve(&self, now: &Zoned) -> Result<Zoned, jiff::Error> {
        match self {
            Self::Fixed(time) => Ok(time.clone()),
            Self::Relative(span) => now.checked_add(span),
        }
    }
}

pub fn parse_unsigned_duration(value: &str) -> Result<Duration, jiff::Error> {
    let body = value.strip_prefix(['+', '-']).unwrap_or(value);
    if body.starts_with(['P', 'p']) {
        jiff::fmt::temporal::SpanParser::new().parse_unsigned_duration(value)
    } else {
        jiff::fmt::friendly::SpanParser::new().parse_unsigned_duration(value)
    }
}
