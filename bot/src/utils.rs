use crate::{api, prelude::*};
use parking_lot::Mutex;
use std::{borrow, fmt, mem, sync::Arc, time};
use url::percent_encoding::PercentDecode;

mod duration;

pub type Futures<'a> = Vec<future::BoxFuture<'a, Result<(), failure::Error>>>;

pub trait Driver<'a> {
    /// Drive the given future.
    fn drive<F>(&mut self, future: F)
    where
        F: 'a + Send + Future<Output = Result<(), failure::Error>>;
}

impl<'a> Driver<'a> for Vec<future::BoxFuture<'a, Result<(), failure::Error>>> {
    fn drive<F>(&mut self, future: F)
    where
        F: 'a + Send + Future<Output = Result<(), failure::Error>>,
    {
        self.push(future.boxed());
    }
}

pub use self::duration::Duration;

pub struct Urls<'a> {
    message: &'a str,
}

impl<'a> Urls<'a> {
    /// Extract all URLs from the given message.
    pub fn new(message: &'a str) -> Urls<'a> {
        Urls {
            message: message.trim(),
        }
    }
}

impl<'a> Iterator for Urls<'a> {
    type Item = url::Url;

    fn next(&mut self) -> Option<Self::Item> {
        while !self.message.is_empty() {
            let index = match self.message.find("http") {
                Some(index) => index,
                None => break,
            };

            let m = &self.message[index..];

            let end = match m.find(|c| !is_url_character(c)) {
                Some(index) => index,
                None => m.len(),
            };

            let (url, rest) = m.split_at(end);
            self.message = rest.trim();

            if let Ok(url) = str::parse::<url::Url>(url) {
                return Some(url);
            }
        }

        None
    }
}

/// Decode a query string.
pub fn query_pairs(query: &str) -> QueryPairs<'_> {
    QueryPairs { query }
}

pub struct QueryPairs<'a> {
    query: &'a str,
}

impl<'a> Iterator for QueryPairs<'a> {
    type Item = (PercentDecode<'a>, Option<PercentDecode<'a>>);

    fn next(&mut self) -> Option<Self::Item> {
        use url::percent_encoding::percent_decode;

        while !self.query.is_empty() {
            let s = match self.query.find('&') {
                Some(index) => {
                    let (s, rest) = self.query.split_at(index);
                    self.query = &rest[1..];
                    s
                }
                None => mem::replace(&mut self.query, ""),
            };

            match s.find('=') {
                Some(index) => {
                    let (s, rest) = s.split_at(index);
                    let key = percent_decode(s.as_bytes());
                    let value = percent_decode(rest[1..].as_bytes());
                    return Some((key, Some(value)));
                }
                None => {
                    let s = percent_decode(s.as_bytes());
                    return Some((s, None));
                }
            }
        }

        None
    }
}

#[derive(Clone, Debug)]
pub struct Words<'a> {
    string: &'a str,
}

impl<'a> Words<'a> {
    /// Split the commandline.
    pub fn new(string: &str) -> Words<'_> {
        Words { string }
    }

    /// The rest of the input.
    pub fn rest(&self) -> &'a str {
        self.string
    }
}

impl<'a> Iterator for Words<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        if self.string.is_empty() {
            return None;
        }

        let mut it = self.string.char_indices();
        let len = self.string.len();

        while let Some((s, c)) = it.next() {
            match c {
                ' ' | '\t' | '\r' | '\n' => continue,
                // parse string
                '"' => {
                    let s = it.next().map(|(i, _)| i).unwrap_or(len);
                    let mut e = len;

                    while let Some((i, c)) = it.next() {
                        match c {
                            '"' => {
                                e = usize::min(i, len);
                                break;
                            }
                            _ => (),
                        }
                    }

                    let out = &self.string[s..e];
                    let e = it.next().map(|(i, _)| i).unwrap_or(len);
                    self.string = &self.string[e..];
                    return Some(out);
                }
                _ => {
                    let mut e = len;

                    while let Some((i, c)) = it.next() {
                        match c {
                            ' ' | '\t' | '\r' | '\n' => {
                                e = usize::min(i, len);
                                break;
                            }
                            _ => (),
                        }
                    }

                    let (head, tail) = self.string.split_at(e);
                    self.string = tail;
                    return Some(&head[s..]);
                }
            }
        }

        self.string = "";
        None
    }
}

#[derive(Debug)]
pub struct TrimmedWords<'a> {
    string: &'a str,
}

impl<'a> TrimmedWords<'a> {
    /// Split the commandline.
    pub fn new(string: &str) -> TrimmedWords<'_> {
        TrimmedWords {
            string: string.trim_start_matches(is_not_alphanum),
        }
    }
}

impl<'a> Iterator for TrimmedWords<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        if self.string.is_empty() {
            return None;
        }

        let (out, rest) = match self.string.find(is_not_alphanum) {
            Some(n) => self.string.split_at(n),
            None => return Some(mem::replace(&mut self.string, "")),
        };

        self.string = rest.trim_start_matches(is_not_alphanum);
        Some(out)
    }
}

/// Test if char is not alphanumeric.
fn is_not_alphanum(c: char) -> bool {
    match c {
        'a'..='z' => false,
        'A'..='Z' => false,
        '0'..='9' => false,
        _ => true,
    }
}

struct DurationParts {
    seconds: u64,
    minutes: u64,
    hours: u64,
    days: u64,
}

/// Partition the given duration into time components.
#[inline(always)]
fn partition(seconds: u64) -> DurationParts {
    let rest = seconds as u64;
    let days = rest / (3600 * 24);
    let rest = rest % (3600 * 24);
    let hours = rest / 3600;
    let rest = rest % 3600;
    let minutes = rest / 60;
    let seconds = rest % 60;

    DurationParts {
        seconds,
        minutes,
        hours,
        days,
    }
}

/// Format the given number of seconds as a compact human time.
pub fn compact_duration(duration: &time::Duration) -> String {
    let mut parts = Vec::new();

    let p = partition(duration.as_secs());

    parts.extend(match p.hours {
        0 => None,
        n => Some(format!("{:02}h", n)),
    });

    parts.extend(match p.minutes {
        0 => None,
        n => Some(format!("{:02}m", n)),
    });

    parts.extend(match p.seconds {
        0 => None,
        n => Some(format!("{:02}s", n)),
    });

    if parts.is_empty() {
        return String::from("0s");
    }

    parts.join(" ")
}

/// Format the given number of seconds as a long human time.
pub fn long_duration(duration: &time::Duration) -> String {
    let mut parts = Vec::new();

    let p = partition(duration.as_secs());

    parts.extend(match p.hours {
        0 => None,
        1 => Some(format!("one hour")),
        n => Some(format!("{} hours", english_num(n))),
    });

    parts.extend(match p.minutes {
        0 => None,
        1 => Some(format!("one minute")),
        n => Some(format!("{} minutes", english_num(n))),
    });

    parts.extend(match p.seconds {
        0 => None,
        1 => Some(format!("one second")),
        n => Some(format!("{} seconds", english_num(n))),
    });

    if parts.is_empty() {
        return String::from("0 seconds");
    }

    parts.join(", ")
}

/// Format the given number of seconds as a digital duration.
pub fn digital_duration(duration: &time::Duration) -> String {
    let mut parts = Vec::new();

    let p = partition(duration.as_secs());

    parts.extend(match p.hours {
        0 => None,
        n => Some(format!("{:02}", n)),
    });

    parts.push(format!("{:02}", p.minutes));
    parts.push(format!("{:02}", p.seconds));

    parts.join(":")
}

/// Format the given number as a string according to english conventions.
pub fn english_num(n: u64) -> borrow::Cow<'static, str> {
    let n = match n {
        1 => "one",
        2 => "two",
        3 => "three",
        4 => "four",
        5 => "five",
        6 => "six",
        7 => "seven",
        8 => "eight",
        9 => "nine",
        n => return borrow::Cow::from(n.to_string()),
    };

    borrow::Cow::Borrowed(n)
}

/// Render artists in a human readable form INCLUDING an oxford comma.
pub fn human_artists(artists: &[api::spotify::SimplifiedArtist]) -> Option<String> {
    if artists.is_empty() {
        return None;
    }

    let mut it = artists.iter();
    let mut artists = String::new();

    if let Some(artist) = it.next() {
        artists.push_str(artist.name.as_str());
    }

    let back = it.next_back();

    while let Some(artist) = it.next() {
        artists.push_str(", ");
        artists.push_str(artist.name.as_str());
    }

    if let Some(artist) = back {
        artists.push_str(", and ");
        artists.push_str(artist.name.as_str());
    }

    Some(artists)
}

/// Test if character is a URL character.
fn is_url_character(c: char) -> bool {
    match c {
        'a'..='z' => true,
        'A'..='Z' => true,
        // url-safe characters
        '$' | '-' | '_' | '.' | '+' | '!' | '*' | '\'' | '(' | ')' => true,
        // control characters.
        ';' | '/' | '?' | ':' | '@' | '=' | '&' => true,
        _ => false,
    }
}

/// An offset with millisecond precision.
///
/// Stored field is in milliseconds.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Offset(u32);

impl std::str::FromStr for Offset {
    type Err = failure::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (s, ms) = match s.rfind('.') {
            Some(i) => (&s[..i], str::parse::<u32>(&s[(i + 1)..])?),
            None => (s, 0),
        };

        let (s, seconds) = match s.rfind(':') {
            Some(i) => (&s[..i], str::parse::<u32>(&s[(i + 1)..])? * 1_000),
            None => ("", str::parse::<u32>(s)? * 1_000),
        };

        let minutes = match s {
            "" => 0,
            s => str::parse::<u32>(s)? * 60_000,
        };

        Ok(Offset(
            ms.checked_add(seconds)
                .and_then(|t| t.checked_add(minutes))
                .unwrap_or_default(),
        ))
    }
}

impl<'de> serde::Deserialize<'de> for Offset {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        str::parse(&s).map_err(serde::de::Error::custom)
    }
}

impl serde::Serialize for Offset {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.collect_str(self)
    }
}

impl Offset {
    /// An offset from milliseconds.
    pub fn milliseconds(ms: u32) -> Self {
        Offset(ms)
    }

    /// Convert to seconds.
    pub fn as_milliseconds(&self) -> u32 {
        self.0
    }

    /// Treat offset as duration.
    pub fn as_duration(&self) -> time::Duration {
        time::Duration::from_millis(self.0 as u64)
    }
}

impl fmt::Display for Offset {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        let rest = self.0;
        let ms = rest % 1_000;
        let rest = rest / 1_000;
        let seconds = rest % 60;
        let minutes = rest / 60;

        if ms > 0 {
            write!(fmt, "{:02}:{:02}.{:03}", minutes, seconds, ms)
        } else {
            write!(fmt, "{:02}:{:02}", minutes, seconds)
        }
    }
}

/// A cooldown implementation that prevents an action from being executed too frequently.
#[derive(Debug, Clone, Default)]
pub struct Cooldown {
    last_action_at: Option<time::Instant>,
    pub cooldown: Duration,
}

impl Cooldown {
    /// Create a cooldown from the given duration.
    pub fn from_duration(duration: Duration) -> Self {
        Self {
            last_action_at: None,
            cooldown: duration,
        }
    }

    /// Test if we are allowed to perform the action based on the cooldown in effect.
    pub fn is_open(&mut self) -> bool {
        let now = time::Instant::now();

        match self.check(now.clone()) {
            None => {
                self.poke(now);
                true
            }
            Some(..) => false,
        }
    }

    /// Test how much time remains until cooldown is open.
    pub fn check(&mut self, now: time::Instant) -> Option<time::Duration> {
        if let Some(last_action_at) = self.last_action_at.as_ref() {
            let since_last_action = now - *last_action_at;
            let cooldown = self.cooldown.as_std();

            if since_last_action < cooldown {
                return Some(cooldown - since_last_action);
            }
        }

        None
    }

    /// Poke the cooldown with the current time
    pub fn poke(&mut self, now: time::Instant) {
        self.last_action_at = Some(now);
    }
}

impl serde::Serialize for Cooldown {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.cooldown.serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for Cooldown {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let duration = Duration::deserialize(deserializer)?;
        Ok(Cooldown::from_duration(duration))
    }
}

/// Helper to handle shutdowns.
#[derive(Clone)]
pub struct Shutdown {
    sender: Arc<Mutex<Option<oneshot::Sender<()>>>>,
}

impl Shutdown {
    /// Construct a new shutdown handler.
    pub fn new() -> (Self, oneshot::Receiver<()>) {
        let (tx, rx) = oneshot::channel();
        (
            Self {
                sender: Arc::new(Mutex::new(Some(tx))),
            },
            rx,
        )
    }

    /// Execute the shutdown handler.
    pub fn shutdown(&self) -> bool {
        if let Some(sender) = self.sender.lock().take() {
            sender.send(()).expect("no listener");
            return true;
        }

        log::warn!("shutdown already called");
        false
    }
}

/// PT-formatted duration.
#[derive(Debug, Clone)]
pub struct PtDuration(time::Duration);

impl PtDuration {
    /// Access the inner duration.
    pub fn as_std(&self) -> &time::Duration {
        &self.0
    }

    /// Convert into inner duration.
    pub fn into_std(self) -> time::Duration {
        self.0
    }
}

impl std::str::FromStr for PtDuration {
    type Err = failure::Error;

    fn from_str(duration: &str) -> Result<Self, Self::Err> {
        let duration = duration.trim_start_matches("PT");

        let (duration, hours) = match duration.find('H') {
            Some(index) => {
                let hours = str::parse::<u64>(&duration[..index])?;
                (&duration[(index + 1)..], hours)
            }
            None => (duration, 0u64),
        };

        let (duration, minutes) = match duration.find('M') {
            Some(index) => {
                let minutes = str::parse::<u64>(&duration[..index])?;
                (&duration[(index + 1)..], minutes)
            }
            None => (duration, 0u64),
        };

        let mut milliseconds = 0;

        let (_, mut seconds) = match duration.find('S') {
            Some(index) => {
                let seconds = &duration[..index];

                let seconds = match seconds.find('.') {
                    Some(dot) => {
                        let (seconds, tail) = seconds.split_at(dot);
                        milliseconds = str::parse::<u64>(&tail[1..])?;
                        seconds
                    }
                    None => seconds,
                };

                let seconds = str::parse::<u64>(seconds)?;
                (&duration[(index + 1)..], seconds)
            }
            None => (duration, 0u64),
        };

        seconds += minutes * 60;
        seconds += hours * 3600;
        milliseconds += seconds * 1000;

        Ok(PtDuration(time::Duration::from_millis(milliseconds)))
    }
}

impl fmt::Display for PtDuration {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        let p = partition(self.0.as_secs());

        write!(fmt, "PT")?;

        if p.hours > 0 {
            write!(fmt, "{}H", p.hours)?;
        }

        if p.minutes > 0 {
            write!(fmt, "{}M", p.minutes)?;
        }

        if p.seconds > 0 {
            write!(fmt, "{}S", p.seconds)?;
        }

        Ok(())
    }
}

impl<'de> serde::Deserialize<'de> for PtDuration {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        str::parse(&s).map_err(serde::de::Error::custom)
    }
}

impl serde::Serialize for PtDuration {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.collect_str(self)
    }
}

#[cfg(test)]
mod tests {
    use super::{Offset, TrimmedWords, Urls, Words};

    #[test]
    pub fn test_offset() -> Result<(), failure::Error> {
        assert_eq!(Offset::milliseconds(1_000), str::parse::<Offset>("1")?);
        assert_eq!(Offset::milliseconds(1_000), str::parse::<Offset>("01")?);
        assert_eq!(Offset::milliseconds(61_000), str::parse::<Offset>("01:01")?);
        assert_eq!(
            Offset::milliseconds(61_123),
            str::parse::<Offset>("01:01.123")?
        );
        Ok(())
    }

    #[test]
    pub fn test_trimmed_words() {
        let out = TrimmedWords::new("hello, do you feel alive?").collect::<Vec<_>>();
        assert_eq!(out, vec!["hello", "do", "you", "feel", "alive"]);
    }

    #[test]
    pub fn test_split_escape() {
        let out = Words::new("   foo bar   baz   ").collect::<Vec<_>>();
        assert_eq!(out, vec!["foo", "bar", "baz"]);
    }

    #[test]
    pub fn test_split_quoted() {
        let out = Words::new("   foo bar   \"baz  biz\" ").collect::<Vec<_>>();
        assert_eq!(out, vec!["foo", "bar", "baz  biz"]);
    }

    #[test]
    pub fn test_urls() {
        let u: Vec<url::Url> =
            Urls::new("here:https://google.se/test+this, and this:http://example.com").collect();

        assert_eq!(
            u,
            vec![
                str::parse("https://google.se/test+this").unwrap(),
                str::parse("http://example.com").unwrap()
            ],
        );
    }
}
