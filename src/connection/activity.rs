//! What programs in a terminal report about themselves: OSC 7501 status records, OSC 9;4
//! progress, the bell, the foreground process, the terminal title, and OSC 9 / OSC 777 desktop
//! notifications. All of it arrives as the terminal creator's `block_event`s, or from its
//! `program_status`, `process`, `title` and `pwd` methods.
use super::titles::Place;
use serde_json::Value;

pub const TERMINAL: &str = "com.superlogical.terminal";

/// What the client reads back from a terminal, outside its live events.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Probe {
    Status,
    Title,
    Process,
    Pwd,
}

impl Probe {
    /// The order a terminal's probes run in.
    pub const ALL: [Self; 4] = [Self::Status, Self::Title, Self::Process, Self::Pwd];

    /// The terminal method that answers this probe.
    pub fn method(self) -> String {
        let name = match self {
            Self::Status => "program_status",
            Self::Title => "title",
            Self::Process => "process",
            Self::Pwd => "pwd",
        };
        format!("{TERMINAL}.{name}")
    }
}

/// A record's state, in priority order: a tab shows the highest state among its records.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ProgramState {
    #[default]
    Idle,
    Done,
    Working,
    Error,
    Blocked,
}

impl ProgramState {
    /// Unknown states read as idle, so a newer server's states never raise an alarm.
    fn parse(value: &str) -> Self {
        match value {
            "working" => Self::Working,
            "done" => Self::Done,
            "blocked" => Self::Blocked,
            "error" => Self::Error,
            _ => Self::Idle,
        }
    }
}

/// Why a blocked program is waiting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockedKind {
    Permission,
    Question,
    Auth,
}

/// A terminal's OSC 9;4 progress report, with its percentage when the program gave one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalProgress {
    Set(Option<u8>),
    Indeterminate,
    Paused(Option<u8>),
    Error(Option<u8>),
}

impl TerminalProgress {
    /// Which report a tab shows when several of its terminals report: errors, then pauses,
    /// then progress.
    pub fn rank(self) -> u8 {
        match self {
            Self::Error(_) => 2,
            Self::Paused(_) => 1,
            Self::Set(_) | Self::Indeterminate => 0,
        }
    }
}

/// The terminal's foreground process when a status report arrived.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Owner {
    pub pid: u64,
    pub name: String,
}

/// An RFC 3339 time by the server's clock, as UTC seconds and nanoseconds. Only compare it
/// with another time from the same server: this computer's clock may disagree.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ServerTime {
    seconds: i64,
    nanos: u32,
}

impl ServerTime {
    /// `YYYY-MM-DDTHH:MM:SS[.fraction](Z|±HH:MM)`; none for anything else.
    pub fn parse(text: &str) -> Option<Self> {
        let b = text.as_bytes();
        let number = |range: std::ops::Range<usize>| -> Option<i64> {
            let digits = b.get(range)?;
            digits.iter().try_fold(0i64, |n, d| {
                d.is_ascii_digit().then(|| n * 10 + i64::from(d - b'0'))
            })
        };
        if b.len() < 20
            || b[4] != b'-'
            || b[7] != b'-'
            || !matches!(b[10], b'T' | b't' | b' ')
            || b[13] != b':'
            || b[16] != b':'
        {
            return None;
        }
        let (year, month, day) = (number(0..4)?, number(5..7)?, number(8..10)?);
        let (hour, minute, second) = (number(11..13)?, number(14..16)?, number(17..19)?);
        let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
        let days_in_month = match month {
            2 if leap => 29,
            2 => 28,
            4 | 6 | 9 | 11 => 30,
            1..=12 => 31,
            _ => return None,
        };
        // RFC 3339 allows a leap second, 60.
        if day < 1 || day > days_in_month || hour > 23 || minute > 59 || second > 60 {
            return None;
        }
        let mut at = 19;
        let mut nanos = 0u32;
        if b[at] == b'.' {
            let digits = b[at + 1..]
                .iter()
                .take_while(|d| d.is_ascii_digit())
                .count();
            if digits == 0 {
                return None;
            }
            for (i, d) in b[at + 1..at + 1 + digits].iter().take(9).enumerate() {
                nanos += u32::from(d - b'0') * 10u32.pow(8 - i as u32);
            }
            at += 1 + digits;
        }
        let offset = match b.get(at..)? {
            [b'Z' | b'z'] => 0,
            [sign @ (b'+' | b'-'), _, _, b':', _, _] => {
                let (h, m) = (number(at + 1..at + 3)?, number(at + 4..at + 6)?);
                if h > 23 || m > 59 {
                    return None;
                }
                (h * 60 + m) * 60 * if *sign == b'-' { -1 } else { 1 }
            }
            _ => return None,
        };
        // Days since 1970-01-01 in the proleptic Gregorian calendar (Howard Hinnant's
        // days_from_civil).
        let y = if month <= 2 { year - 1 } else { year };
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let doy = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        let days = era * 146_097 + doe - 719_468;
        Some(Self {
            seconds: days * 86_400 + hour * 3600 + minute * 60 + second - offset,
            nanos,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct StatusRecord {
    pub state: ProgramState,
    pub kind: Option<BlockedKind>,
    /// The program that reported, as `effective_app` (or `app`) names it.
    pub app: Option<String>,
    /// The foreground process when the report arrived; names the program without an app.
    pub owner: Option<Owner>,
    /// When the report arrived, by the server's clock: orders this server's records only.
    pub updated_at: Option<ServerTime>,
    /// The program's own words; shown only as plain text.
    pub title: Option<String>,
    pub msg: Option<String>,
    /// How far along the program says it is, 0–100; only working and blocked records have one.
    pub progress: Option<u8>,
    /// Reported while this client was listening, not read back by a query: only live
    /// changes are news worth a notification.
    pub live: bool,
}

impl StatusRecord {
    pub fn parse(value: &Value) -> Self {
        let text = |key: &str| {
            value[key]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
        };
        Self {
            state: ProgramState::parse(value["state"].as_str().unwrap_or_default()),
            kind: match value["kind"].as_str() {
                Some("permission") => Some(BlockedKind::Permission),
                Some("question") => Some(BlockedKind::Question),
                Some("auth") => Some(BlockedKind::Auth),
                _ => None,
            },
            app: text("effective_app").or_else(|| text("app")),
            owner: value["owner"]["pid"].as_u64().and_then(|pid| {
                let name = value["owner"]["name"].as_str().filter(|s| !s.is_empty())?;
                Some(Owner {
                    pid,
                    name: name.to_owned(),
                })
            }),
            updated_at: value["updated_at"].as_str().and_then(ServerTime::parse),
            title: text("title"),
            msg: text("msg"),
            progress: value["progress"]
                .as_u64()
                .filter(|p| *p <= 100)
                .map(|p| p as u8),
            live: false,
        }
    }

    /// The program behind the record: its app, else the process that was in the foreground
    /// when it reported.
    pub fn program(&self) -> Option<&str> {
        self.app
            .as_deref()
            .or(self.owner.as_ref().map(|o| o.name.as_str()))
    }
}

/// The name to show for a foreground process: the basename of the first of `script`,
/// `argv0` and `name` that is present, without a login shell's leading `-`.
pub fn program_name(foreground: &Value) -> Option<String> {
    ["script", "argv0", "name"]
        .iter()
        .filter_map(|key| foreground[*key].as_str())
        .find(|s| !s.is_empty())
        .map(|s| s.rsplit('/').next().unwrap_or(s).trim_start_matches('-'))
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

#[derive(Clone, Debug, PartialEq)]
pub enum BlockEvent {
    StatusChanged {
        id: String,
        record: StatusRecord,
    },
    StatusRemoved {
        ids: Vec<String>,
    },
    /// The foreground program's name, and the pids of it and its ancestors; empty when the
    /// server couldn't tell.
    Process {
        name: Option<String>,
        pids: Vec<u64>,
        /// Foreground directory and the terminal's own shell, for display-only titles.
        place: Place,
    },
    /// A program set the terminal's title (empty: cleared), with the pid that set it.
    Title {
        text: String,
        owner: Option<u64>,
    },
    Notification {
        title: String,
        body: String,
    },
    /// An OSC 9;4 report; none when the program removed its progress.
    Progress(Option<TerminalProgress>),
    /// The terminal's working directory as a path; none when the report leaves it out.
    Pwd(Option<String>),
    /// The program rang the terminal's bell (BEL).
    Bell,
}

/// A terminal `block_event`'s block and meaning; none for other creators and other events.
pub fn parse_block_event(payload: &Value) -> Option<(String, BlockEvent)> {
    if payload["creator_name"] != TERMINAL {
        return None;
    }
    let block = payload["block_id"].as_str()?.to_owned();
    let body = &payload["payload"];
    let event = match payload["name"].as_str()? {
        "program_status_changed" => BlockEvent::StatusChanged {
            id: body["id"].as_str().unwrap_or_default().to_owned(),
            record: StatusRecord {
                live: true,
                ..StatusRecord::parse(&body["record"])
            },
        },
        "program_status_removed" => BlockEvent::StatusRemoved {
            ids: body["ids"]
                .as_array()?
                .iter()
                .filter_map(|id| id.as_str().map(str::to_owned))
                .collect(),
        },
        "process_changed" => parse_process(body),
        "title_changed" => parse_title(body),
        "pwd_changed" => parse_pwd(body),
        "desktop_notification" => BlockEvent::Notification {
            title: body["title"].as_str().unwrap_or_default().to_owned(),
            body: body["body"].as_str().unwrap_or_default().to_owned(),
        },
        "bell" => BlockEvent::Bell,
        "progress_report" => BlockEvent::Progress({
            let percent = body["progress"]
                .as_u64()
                .filter(|p| *p <= 100)
                .map(|p| p as u8);
            match body["state"].as_str()? {
                "remove" => None,
                "set" => Some(TerminalProgress::Set(percent)),
                "indeterminate" => Some(TerminalProgress::Indeterminate),
                "pause" => Some(TerminalProgress::Paused(percent)),
                "error" => Some(TerminalProgress::Error(percent)),
                _ => return None,
            }
        }),
        _ => return None,
    };
    Some((block, event))
}

/// A `process_changed` payload or a `process` reply.
pub fn parse_process(body: &Value) -> BlockEvent {
    let foreground = &body["foreground"];
    let pids = std::iter::once(foreground)
        .chain(body["ancestors"].as_array().into_iter().flatten())
        .filter_map(|p| p["pid"].as_u64())
        .collect();
    BlockEvent::Process {
        name: program_name(foreground),
        pids,
        place: Place::parse(body),
    }
}

/// A `title_changed` payload or a `title` reply.
pub fn parse_title(body: &Value) -> BlockEvent {
    BlockEvent::Title {
        text: body["title"].as_str().unwrap_or_default().trim().to_owned(),
        owner: body["owner"]["pid"].as_u64(),
    }
}

/// A `pwd_changed` payload or a `pwd` reply.
pub fn parse_pwd(body: &Value) -> BlockEvent {
    BlockEvent::Pwd(body["pwd"].as_str().and_then(cwd_path))
}

/// The path in a reported working directory: a `file://host/path` URL's path with its `%XX`
/// escapes decoded (an escape that isn't `%` and two hex digits stays as written), or a plain
/// path unchanged. None when there's no path, as in `file://host`.
pub fn cwd_path(reported: &str) -> Option<String> {
    let Some(rest) = reported.strip_prefix("file://") else {
        return (!reported.is_empty()).then(|| reported.to_owned());
    };
    let path = rest.find('/').map_or("", |at| &rest[at..]);
    let bytes = path.as_bytes();
    let hex = |b: u8| char::from(b).to_digit(16);
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let escaped = match bytes[i..] {
            [b'%', high, low, ..] => hex(high).zip(hex(low)).map(|(h, l)| (h * 16 + l) as u8),
            _ => None,
        };
        match escaped {
            Some(byte) => {
                out.push(byte);
                i += 3;
            }
            None => {
                out.push(bytes[i]);
                i += 1;
            }
        }
    }
    (!out.is_empty()).then(|| String::from_utf8_lossy(&out).into_owned())
}

/// A `program_status` reply's records by id; the root record's id is empty.
pub fn parse_status(reply: &Value) -> Vec<(String, StatusRecord)> {
    reply["records"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|r| {
            (
                r["id"].as_str().unwrap_or_default().to_owned(),
                StatusRecord::parse(r),
            )
        })
        .collect()
}

/// The shell's directory from a server `process` reply, even without OSC 7. Prefer the
/// terminal's child, not an editor's foreground directory. macOS wraps the shell in login;
/// use the process just below it only when the ancestry is available.
pub fn shell_dir(process: &Value) -> Option<String> {
    let mut shell = &process["child"];
    if shell["name"] == "login" {
        shell = match process["ancestors"].as_array().map(Vec::as_slice) {
            Some([.., below, _login]) => below,
            Some([_login]) => &process["foreground"],
            _ => return None,
        };
        if shell["name"] == "login" {
            return None;
        }
    }
    shell["cwd"]
        .as_str()
        .filter(|dir| dir.starts_with('/'))
        .map(str::to_owned)
}

/// An OSC 9 or OSC 777 notification a program sent, with where it came from.
#[derive(Clone, Debug, PartialEq)]
pub struct DesktopNotice {
    pub session: String,
    pub block: String,
    pub title: String,
    pub body: String,
}
