pub mod interface;
pub mod recorded;
pub mod shortcuts;
pub mod terminal;
pub mod typography;

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Host {
    pub label: String,
    pub endpoint: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub remote_control: bool,
    /// Keep built-in Tailscale on and named whenever this host connects.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub keep_tailscale: bool,
    /// The Tailscale device name to use; none means the default for this machine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tailscale_name: Option<String>,
    #[serde(default)]
    pub collapsed: bool,
    /// The shell new terminals on this host start (`zsh`, `/bin/zsh`); none uses the server
    /// user's login shell.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shell: Option<String>,
}

impl Host {
    /// Whether this client may control the host's Tailscale: only a local Unix socket.
    pub fn controls_tailscale(&self) -> bool {
        self.endpoint.starts_with("unix://")
    }

    /// The device name to use: the saved one, or this machine's default when none is saved.
    /// An invalid saved name is an error, never replaced by the default.
    pub fn tailscale_name(&self) -> Result<String, String> {
        use crate::connection::tailscale::{default_name, valid_name};
        match &self.tailscale_name {
            Some(name) if valid_name(name) => Ok(name.clone()),
            Some(name) => Err(format!(
                "The saved device name \u{201c}{name}\u{201d} isn't valid. Edit it to use Tailscale from here."
            )),
            None => Ok(default_name(&gethostname::gethostname().to_string_lossy())),
        }
    }

    /// The name every automatic path sends: set only when this host opts in and the name is valid.
    pub fn kept_tailscale(&self) -> Option<String> {
        if self.controls_tailscale() && self.keep_tailscale {
            self.tailscale_name().ok()
        } else {
            None
        }
    }
}

/// A tab this client keeps quiet: no activity word or progress bar on it and no desktop
/// notification from it. Local to this client; the server and other clients never see it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MutedTab {
    /// The saved host's endpoint.
    pub endpoint: String,
    pub session: String,
    /// The Rex window id, unique on its server.
    pub window: String,
}

/// The window corner in-app notices stack in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoticePosition {
    #[default]
    TopRight,
    BottomRight,
}

/// The space and decorations around terminal panes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaneDensity {
    Compact,
    #[default]
    Comfortable,
}

/// When a pane shows its header: its name, its program's activity and the Split, Zoom and
/// Close buttons. A value this version doesn't know reads as Auto.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaneHeader {
    /// On every pane, a single one included, in either density.
    Always,
    /// Over the top of a pane while the pointer rests there; the terminal keeps its size.
    OnHover,
    Hidden,
    /// With the density: on the cards of a split or zoomed tab in Comfortable, never in
    /// Compact, and a single pane stays edge to edge.
    #[default]
    #[serde(other)]
    Auto,
}

/// How a tab arranges its panes: tiled to the window, or as columns that scroll sideways.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TabLayout {
    #[default]
    Tiled,
    Scrolling,
}

/// When focus movement centers a column in a scrolling tab.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FocusCentering {
    #[default]
    Never,
    OnOverflow,
    Always,
}

/// How the Activity view groups its rows: by urgency, or by session.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityGrouping {
    #[default]
    Priority,
    Session,
}

/// What the sidebar lists: hosts and their sessions, or the panes reporting activity.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SidebarMode {
    #[default]
    Sessions,
    Activity,
}

/// How Sessions mode lists sessions: under a header per host, or in one list with the host
/// as a prefix while two or more hosts are connected.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionList {
    #[default]
    ByHost,
    Flat,
}

/// Client-local column widths, keyed by host endpoint, session, window and column.
pub type ColumnWidths = HashMap<String, HashMap<String, HashMap<String, HashMap<String, f64>>>>;

/// Where one program state shows: a word (or progress bar) in tabs, a desktop notification,
/// the unread count, and a sound with the notification.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Channels {
    pub tab: bool,
    pub desktop: bool,
    pub badge: bool,
    pub sound: bool,
}

/// Notifications programs send themselves (OSC 9, OSC 777): a desktop notification and its
/// sound only.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageChannels {
    pub desktop: bool,
    pub sound: bool,
}

/// What program activity shows where, state by state: in tabs, as desktop notifications, in
/// the unread count and as sounds. Saved in this shape; files from before it migrate (see
/// [`LegacyNotifications`]) so a user's choices behave as they did.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "LegacyNotifications")]
pub struct NotificationsConfig {
    /// Working shows as a progress bar in tabs; it never counts as unread.
    pub working: Channels,
    pub blocked: Channels,
    pub error: Channels,
    pub done: Channels,
    pub messages: MessageChannels,
}
impl Default for NotificationsConfig {
    fn default() -> Self {
        let on = Channels {
            tab: true,
            desktop: true,
            badge: true,
            sound: false,
        };
        Self {
            working: Channels {
                desktop: false,
                badge: false,
                ..on
            },
            blocked: on,
            error: on,
            done: on,
            messages: MessageChannels {
                desktop: true,
                sound: false,
            },
        }
    }
}
impl NotificationsConfig {
    /// Every state's channels, in the order Settings lists them.
    pub fn states(&self) -> [(crate::connection::activity::ProgramState, Channels); 4] {
        use crate::connection::activity::ProgramState::*;
        [
            (Blocked, self.blocked),
            (Error, self.error),
            (Done, self.done),
            (Working, self.working),
        ]
    }

    /// A state's channels; Idle has none.
    pub fn channels(&self, state: crate::connection::activity::ProgramState) -> Option<Channels> {
        use crate::connection::activity::ProgramState::*;
        match state {
            Working => Some(self.working),
            Blocked => Some(self.blocked),
            Error => Some(self.error),
            Done => Some(self.done),
            Idle => None,
        }
    }

    pub fn channels_mut(
        &mut self,
        state: crate::connection::activity::ProgramState,
    ) -> Option<&mut Channels> {
        use crate::connection::activity::ProgramState::*;
        match state {
            Working => Some(&mut self.working),
            Blocked => Some(&mut self.blocked),
            Error => Some(&mut self.error),
            Done => Some(&mut self.done),
            Idle => None,
        }
    }

    /// Any state shows in tabs. A bell's word follows this, having no state of its own.
    pub fn any_tab(&self) -> bool {
        self.states().iter().any(|(_, c)| c.tab)
    }

    /// Turn the tab channel on or off for every state, as the old single switch did.
    pub fn set_tabs(&mut self, on: bool) {
        for c in [
            &mut self.working,
            &mut self.blocked,
            &mut self.error,
            &mut self.done,
        ] {
            c.tab = on;
        }
    }

    /// Any channel that needs every session's activity: tabs, notifications or the count.
    pub fn watches_activity(&self) -> bool {
        self.states()
            .iter()
            .any(|(_, c)| c.tab || c.desktop || c.badge)
    }
}

/// One state as saved: a `Channels` object, or before it the old per-state notification
/// switch.
#[derive(Deserialize)]
#[serde(untagged)]
enum LegacyState {
    Channels(PartialChannels),
    Switch(bool),
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct PartialChannels {
    tab: Option<bool>,
    desktop: Option<bool>,
    badge: Option<bool>,
    sound: Option<bool>,
}

/// Either notification shape. Before the channels table, `show_in_tabs` switched every tab
/// word, `system_notifications` every desktop notification, `working`, `blocked`, `error`
/// and `done` each state's notification and `program_messages` programs' own. They map to
/// `tab` for every state, `desktop = system_notifications && <state>`, and
/// `messages.desktop = system_notifications && program_messages`. The count is new and
/// shows where tab words did (`badge = show_in_tabs && <default>`); sound is new and off.
#[derive(Default, Deserialize)]
#[serde(default)]
pub struct LegacyNotifications {
    show_in_tabs: Option<bool>,
    system_notifications: Option<bool>,
    program_messages: Option<bool>,
    working: Option<LegacyState>,
    blocked: Option<LegacyState>,
    error: Option<LegacyState>,
    done: Option<LegacyState>,
    messages: Option<PartialMessages>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct PartialMessages {
    desktop: Option<bool>,
    sound: Option<bool>,
}

impl From<LegacyNotifications> for NotificationsConfig {
    fn from(old: LegacyNotifications) -> Self {
        let defaults = Self::default();
        let system = old.system_notifications.unwrap_or(true);
        let state = |saved: Option<LegacyState>, default: Channels| {
            let tab = old.show_in_tabs.unwrap_or(default.tab);
            match saved {
                Some(LegacyState::Channels(c)) => Channels {
                    tab: c.tab.unwrap_or(tab),
                    desktop: c.desktop.unwrap_or(system && default.desktop),
                    badge: c.badge.unwrap_or(default.badge),
                    sound: c.sound.unwrap_or(default.sound),
                },
                Some(LegacyState::Switch(on)) => Channels {
                    tab,
                    desktop: system && on,
                    badge: tab && default.badge,
                    sound: default.sound,
                },
                None => Channels {
                    tab,
                    desktop: system && default.desktop,
                    badge: tab && default.badge,
                    sound: default.sound,
                },
            }
        };
        let messages = old.messages.unwrap_or_default();
        Self {
            working: state(old.working, defaults.working),
            blocked: state(old.blocked, defaults.blocked),
            error: state(old.error, defaults.error),
            done: state(old.done, defaults.done),
            messages: MessageChannels {
                desktop: messages
                    .desktop
                    .unwrap_or(system && old.program_messages.unwrap_or(defaults.messages.desktop)),
                sound: messages.sound.unwrap_or(defaults.messages.sound),
            },
        }
    }
}

/// Whether the client checks for signed release metadata in the background.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdatesConfig {
    pub check: bool,
}
impl Default for UpdatesConfig {
    fn default() -> Self {
        Self { check: true }
    }
}

/// Where downloaded files are saved.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DownloadsConfig {
    /// The folder, absolute or starting with `~`, as the person wrote it. None is the XDG
    /// download folder (`xdg-user-dir DOWNLOAD`, else `~/Downloads`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub directory: Option<String>,
}

/// The palette the chrome is drawn in. A value this version doesn't know is kept as
/// `Unavailable`, drawn as Midnight and saved back unchanged.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ChromeTheme {
    RosePine,
    RosePineMoon,
    GruvboxDark,
    #[default]
    Midnight,
    /// An index into `dracula_pro::THEMES`, a theme with a chrome palette.
    Generated(u8),
    /// A saved id this build can't draw (an older or newer version's, or a generated theme
    /// that isn't in this build).
    Unavailable(&'static str),
}

// By hand: the derive would tie `'de` to `'static` because of `Unavailable`'s string.
impl Serialize for ChromeTheme {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        String::from(*self).serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for ChromeTheme {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer).map(Self::from)
    }
}

/// Leaks each distinct string once, so `ChromeTheme` stays `Copy`.
fn intern(s: String) -> &'static str {
    static SEEN: std::sync::Mutex<Vec<&'static str>> = std::sync::Mutex::new(Vec::new());
    let mut seen = SEEN.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(&known) = seen.iter().find(|&&k| k == s) {
        return known;
    }
    let leaked: &'static str = Box::leak(s.into_boxed_str());
    seen.push(leaked);
    leaked
}

impl From<String> for ChromeTheme {
    fn from(value: String) -> Self {
        match value.as_str() {
            "midnight" => Self::Midnight,
            "rose_pine" => Self::RosePine,
            "rose_pine_moon" => Self::RosePineMoon,
            "gruvbox_dark" => Self::GruvboxDark,
            _ => match crate::dracula_pro::chrome_index(&value) {
                Some(i) => Self::Generated(i),
                None => Self::Unavailable(intern(value)),
            },
        }
    }
}
impl From<ChromeTheme> for String {
    fn from(chrome: ChromeTheme) -> Self {
        match chrome {
            ChromeTheme::Midnight => "midnight".into(),
            ChromeTheme::RosePine => "rose_pine".into(),
            ChromeTheme::RosePineMoon => "rose_pine_moon".into(),
            ChromeTheme::GruvboxDark => "gruvbox_dark".into(),
            ChromeTheme::Generated(i) => {
                crate::dracula_pro::THEMES[i as usize].id.replace('-', "_")
            }
            ChromeTheme::Unavailable(s) => s.into(),
        }
    }
}
impl ChromeTheme {
    /// The four palettes that always ship.
    pub const ALL: [ChromeTheme; 4] = [
        ChromeTheme::Midnight,
        ChromeTheme::RosePine,
        ChromeTheme::RosePineMoon,
        ChromeTheme::GruvboxDark,
    ];
    /// Every palette this build can draw: the shipped four, then each generated theme that
    /// has a chrome.
    pub fn all() -> Vec<ChromeTheme> {
        let generated = crate::dracula_pro::THEMES
            .iter()
            .enumerate()
            .filter(|(_, t)| t.chrome.is_some())
            .map(|(i, _)| ChromeTheme::Generated(i as u8));
        Self::ALL.into_iter().chain(generated).collect()
    }
    /// What to draw: an unavailable palette draws as Midnight.
    pub fn effective(self) -> ChromeTheme {
        match self {
            ChromeTheme::Unavailable(_) => ChromeTheme::Midnight,
            other => other,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            ChromeTheme::Midnight | ChromeTheme::Unavailable(_) => "Midnight",
            ChromeTheme::RosePine => "Rosé Pine",
            ChromeTheme::RosePineMoon => "Rosé Pine Moon",
            ChromeTheme::GruvboxDark => "Gruvbox Dark",
            ChromeTheme::Generated(i) => crate::dracula_pro::THEMES[i as usize].name,
        }
    }
}

/// The terminal theme this client declares to the server, saved as `follow_server`,
/// `builtin_default`, `follow_chrome` or `named:<id>`. An id is a Ghostty theme's file name,
/// or a built-in theme's after `rex/`. A value this version doesn't know reads as Follow
/// server.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub enum TerminalThemeChoice {
    /// Declare nothing: panes show whatever colours the server gives them.
    #[default]
    FollowServer,
    /// Declare Rex's built-in colours.
    RexDefault,
    /// Declare the built-in theme paired with the chrome palette, whose background is the
    /// chrome's ground.
    FollowChrome,
    Named(String),
}
impl From<String> for TerminalThemeChoice {
    fn from(value: String) -> Self {
        match value.as_str() {
            "builtin_default" => Self::RexDefault,
            "follow_chrome" => Self::FollowChrome,
            _ => match value.strip_prefix("named:") {
                Some(id) if !id.is_empty() => Self::Named(id.to_owned()),
                _ => Self::FollowServer,
            },
        }
    }
}
impl From<TerminalThemeChoice> for String {
    fn from(choice: TerminalThemeChoice) -> Self {
        match choice {
            TerminalThemeChoice::FollowServer => "follow_server".into(),
            TerminalThemeChoice::RexDefault => "builtin_default".into(),
            TerminalThemeChoice::FollowChrome => "follow_chrome".into(),
            TerminalThemeChoice::Named(id) => format!("named:{id}"),
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub hosts: Vec<Host>,
    /// None follows the current hostname rather than persisting a snapshot of it.
    pub client_label: Option<String>,
    pub last_host: Option<String>,
    pub sessions: HashMap<String, String>,
    pub sidebar_width: f32,
    /// The sidebar was open when the app last saved, so it opens again at launch.
    pub sidebar_open: bool,
    pub sidebar_mode: SidebarMode,
    pub sidebar_sessions: SessionList,
    /// The last non-maximized window size in window points, independent of interface zoom.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_size: Option<[f32; 2]>,
    pub interface: interface::InterfaceConfig,
    pub vertical_tabs: bool,
    pub pane_density: PaneDensity,
    pub pane_header: PaneHeader,
    pub pane_padding: f32,
    pub font_size: f32,
    pub terminal: terminal::TerminalConfig,
    pub font_family: String,
    pub font_fallbacks: Vec<String>,
    pub ui_scale: f32,
    pub notice_position: NoticePosition,
    pub notifications: NotificationsConfig,
    pub updates: UpdatesConfig,
    pub downloads: DownloadsConfig,
    /// Passed to `rex skill setup` as `--session-naming`, always explicitly.
    pub agent_session_naming: bool,
    pub chrome_theme: ChromeTheme,
    pub terminal_theme: TerminalThemeChoice,
    /// The user picked a terminal theme themselves at least once, so choosing a chrome
    /// palette leaves it alone. Until then a palette brings its paired theme (Follow chrome).
    pub terminal_theme_chosen: bool,
    /// This client has declared a terminal theme to some server at least once. Rex keeps a
    /// client's last declaration per block across launches, so Follow server then resets
    /// eligible blocks to Rex's colours. Never cleared automatically.
    pub terminal_theme_declared: bool,
    /// The layout of tabs without their own choice.
    pub tab_layout: TabLayout,
    pub focus_centering: FocusCentering,
    pub activity_grouping: ActivityGrouping,
    /// The Activity view lists agents that have records but are all idle.
    pub activity_show_idle: bool,
    /// Prefer foreground program and directory over shell-set titles. Display only.
    pub generated_titles: bool,
    /// Tabs given their own layout, by host endpoint, session ID and Rex window ID.
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub tab_layouts: HashMap<String, HashMap<String, HashMap<String, TabLayout>>>,
    /// Widths changed from the half-window default, by host, session, window and column ID.
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub column_widths: ColumnWidths,
    /// Tabs muted from their menu, until unmuted or closed.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub muted_tabs: Vec<MutedTab>,
    /// Shortcuts recorded in Settings → Shortcuts: a keymap layer above `shortcut_file`.
    #[serde(skip_serializing_if = "recorded::Recorded::is_empty")]
    pub recorded_shortcuts: recorded::Recorded,
    /// Read-only sibling file, never serialized into workspace.json.
    #[serde(skip)]
    pub shortcut_file: shortcuts::Shortcuts,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            hosts: vec![Host {
                label: std::fs::read_to_string("/etc/hostname")
                    .unwrap_or_else(|_| "Local".into())
                    .trim()
                    .to_owned(),
                endpoint: crate::transport::local_endpoint(),
                remote_control: false,
                keep_tailscale: false,
                tailscale_name: None,
                collapsed: false,
                shell: None,
            }],
            client_label: None,
            last_host: None,
            sessions: HashMap::new(),
            sidebar_width: 236.,
            sidebar_open: false,
            sidebar_mode: SidebarMode::default(),
            sidebar_sessions: SessionList::default(),
            window_size: None,
            interface: interface::InterfaceConfig::default(),
            vertical_tabs: false,
            pane_density: PaneDensity::default(),
            pane_header: PaneHeader::default(),
            pane_padding: 8.,
            font_size: 15.,
            terminal: Default::default(),
            font_family: "JetBrainsMono Nerd Font".into(),
            font_fallbacks: Vec::new(),
            ui_scale: 1.,
            notice_position: NoticePosition::default(),
            notifications: NotificationsConfig::default(),
            updates: UpdatesConfig::default(),
            downloads: DownloadsConfig::default(),
            agent_session_naming: false,
            chrome_theme: ChromeTheme::default(),
            terminal_theme: TerminalThemeChoice::default(),
            terminal_theme_chosen: false,
            terminal_theme_declared: false,
            tab_layout: TabLayout::default(),
            focus_centering: FocusCentering::default(),
            activity_grouping: ActivityGrouping::default(),
            activity_show_idle: false,
            generated_titles: false,
            tab_layouts: HashMap::new(),
            column_widths: HashMap::new(),
            muted_tabs: Vec::new(),
            recorded_shortcuts: recorded::Recorded::default(),
            shortcut_file: shortcuts::Shortcuts::default(),
        }
    }
}
impl Settings {
    pub fn client_label(&self) -> String {
        self.client_label
            .as_deref()
            .map(str::trim)
            .filter(|label| !label.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(hostname)
    }

    fn config_dir() -> PathBuf {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config")
            })
    }
    pub fn path() -> PathBuf {
        Self::config_dir().join("rex-linux/workspace.json")
    }
    pub fn load_default() -> Result<(Self, PathBuf)> {
        let path = Self::path();
        let settings = Self::load(&path)?;
        Ok((settings, path))
    }
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self {
                shortcut_file: shortcuts::Shortcuts::load(path),
                ..Self::default()
            });
        }
        let value: serde_json::Value = serde_json::from_slice(&std::fs::read(path)?)
            .with_context(|| format!("Unable to read {}", path.display()))?;
        let mut settings: Self = serde_json::from_value(value.clone())
            .with_context(|| format!("Unable to read {}", path.display()))?;
        for host in &settings.hosts {
            validate_host(&host.label, &host.endpoint)?;
        }
        ensure!(
            !settings.hosts.is_empty(),
            "workspace needs at least one host"
        );
        (settings.font_family, settings.font_fallbacks) =
            normalize_font_preferences(&settings.font_family, &settings.font_fallbacks)?;
        settings.font_size = settings.font_size.clamp(8., 48.);
        settings.pane_padding = settings.pane_padding.clamp(0., 32.);
        settings.interface.normalize();
        settings.sidebar_width = settings.sidebar_width.clamp(200., 360.);
        settings.ui_scale = normalize_ui_scale(settings.ui_scale);
        for sessions in settings.column_widths.values_mut() {
            for windows in sessions.values_mut() {
                for columns in windows.values_mut() {
                    for width in columns.values_mut() {
                        *width = width.clamp(
                            crate::layout::MIN_COLUMN_WIDTH,
                            crate::layout::MAX_COLUMN_WIDTH,
                        );
                    }
                }
            }
        }
        // Import before the first save loses nivox's unknown keybindings field.
        let imported = shortcuts::import_nivox(&value, &shortcuts::Shortcuts::path(path));
        settings.shortcut_file = shortcuts::Shortcuts::load(path);
        match imported {
            Ok(count) => settings.shortcut_file.imported = count,
            Err(error) => settings
                .shortcut_file
                .problem(format!("Unable to import nivox shortcuts: {error}")),
        }
        Ok(settings)
    }
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        let temporary = path.with_extension("json.tmp");
        std::fs::write(&temporary, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(temporary, path)?;
        Ok(())
    }
}

pub fn hostname() -> String {
    hostname_label(&gethostname::gethostname().to_string_lossy())
}

fn hostname_label(name: &str) -> String {
    let name = name.trim();
    if name.is_empty() {
        "Rex Linux".into()
    } else {
        name.to_owned()
    }
}

pub fn parse_font_family(family: &str) -> std::result::Result<String, String> {
    let family = family.trim();
    if family.is_empty() || family.chars().any(|c| c.is_control() || c == ',') {
        return Err("Enter one font family name, without commas or control characters.".into());
    }
    Ok(family.to_owned())
}

pub fn normalize_font_preferences(
    family: &str,
    fallbacks: &[String],
) -> Result<(String, Vec<String>)> {
    let family = parse_font_family(family).map_err(anyhow::Error::msg)?;
    let mut names: Vec<String> = Vec::new();
    for fallback in fallbacks {
        let fallback = parse_font_family(fallback).map_err(anyhow::Error::msg)?;
        if !fallback.eq_ignore_ascii_case(&family)
            && !names
                .iter()
                .any(|name| name.eq_ignore_ascii_case(&fallback))
        {
            names.push(fallback);
        }
    }
    Ok((family, names))
}

pub fn normalize_ui_scale(scale: f32) -> f32 {
    if scale.is_finite() {
        (scale * 10.).round().clamp(7., 15.) / 10.
    } else {
        1.
    }
}
pub fn validate_host(label: &str, endpoint: &str) -> Result<()> {
    ensure!(!label.trim().is_empty(), "Enter a host name");
    if let Some(path) = endpoint.strip_prefix("unix://") {
        ensure!(path.starts_with('/'), "Unix socket paths must be absolute");
    } else if crate::ssh::Target::parse(endpoint)?.is_none() {
        let url =
            url::Url::parse(endpoint).context("Use an https://, ssh:// or unix:// endpoint")?;
        ensure!(
            matches!(url.scheme(), "https" | "http" | "ws" | "wss") && url.host_str().is_some(),
            "Use an https://, ssh:// or unix:// endpoint"
        );
    }
    Ok(())
}

/// A name for a host from its endpoint alone: the machine for a URL or SSH target, the socket
/// path otherwise.
pub fn endpoint_label(endpoint: &str) -> String {
    if let Ok(Some(target)) = crate::ssh::Target::parse(endpoint) {
        return target.host;
    }
    if !endpoint.starts_with("unix://")
        && let Ok(url) = url::Url::parse(endpoint)
        && let Some(host) = url.host_str()
    {
        return host.to_owned();
    }
    endpoint.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pane_padding_defaults_clamps_and_round_trips_zero() {
        let old: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(old.pane_padding, 8.);
        let path =
            std::env::temp_dir().join(format!("rex-padding-settings-{}.json", std::process::id()));
        for (padding, expected) in [(-1., 0.), (0., 0.), (12., 12.), (100., 32.)] {
            Settings {
                pane_padding: padding,
                ..Settings::default()
            }
            .save(&path)
            .unwrap();
            assert_eq!(Settings::load(&path).unwrap().pane_padding, expected);
        }
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn font_preferences_survive_a_save_and_reload() {
        let json = serde_json::json!({
            "font_family": "TX02 Nerd Font Mono",
            "font_fallbacks": ["Noto Sans CJK JP", "Noto Color Emoji"]
        });
        let settings: Settings = serde_json::from_value(json.clone()).unwrap();
        let path =
            std::env::temp_dir().join(format!("rex-font-settings-{}.json", std::process::id()));
        settings.save(&path).unwrap();
        let saved = serde_json::to_value(Settings::load(&path).unwrap()).unwrap();
        std::fs::remove_file(path).unwrap();
        assert_eq!(saved["font_family"], json["font_family"]);
        assert_eq!(saved["font_fallbacks"], json["font_fallbacks"]);
    }

    #[test]
    fn font_preferences_default_normalize_and_reject_invalid_names() {
        let old: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(old.font_family, "JetBrainsMono Nerd Font");
        assert!(old.font_fallbacks.is_empty());
        let (family, fallbacks) = normalize_font_preferences(
            "  IBM Plex Mono  ",
            &[
                "Noto Sans CJK JP".into(),
                " noto sans cjk jp ".into(),
                "IBM Plex Mono".into(),
            ],
        )
        .unwrap();
        assert_eq!(family, "IBM Plex Mono");
        assert_eq!(fallbacks, ["Noto Sans CJK JP"]);
        for invalid in ["", "   ", "bad\nname", "two,families"] {
            assert!(
                normalize_font_preferences(invalid, &[]).is_err(),
                "{invalid:?}"
            );
            assert!(normalize_font_preferences("IBM Plex Mono", &[invalid.into()]).is_err());
        }
        assert!(normalize_font_preferences("IBM Plex Mono", &[]).is_ok());
    }

    #[test]
    fn client_label_follows_hostname_unless_explicit_and_loads_existing_strings() {
        for json in [r#"{}"#, r#"{"client_label":null}"#] {
            let settings: Settings = serde_json::from_str(json).unwrap();
            assert!(settings.client_label.is_none());
            assert_eq!(settings.client_label(), hostname());
            let saved = serde_json::to_value(&settings).unwrap();
            assert!(
                saved["client_label"].is_null(),
                "saving must not freeze the hostname"
            );
        }
        let settings: Settings = serde_json::from_str(r#"{"client_label":"My laptop"}"#).unwrap();
        assert_eq!(settings.client_label.as_deref(), Some("My laptop"));
        assert_eq!(settings.client_label(), "My laptop");
        let saved = serde_json::to_value(&settings).unwrap();
        assert_eq!(saved["client_label"], "My laptop");
        // Old files with an empty label still load and get a usable default.
        let empty: Settings = serde_json::from_str(r#"{"client_label":""}"#).unwrap();
        assert_eq!(empty.client_label(), hostname());
        assert_eq!(
            hostname(),
            hostname_label(&gethostname::gethostname().to_string_lossy())
        );
        assert_eq!(hostname_label("  workstation\n"), "workstation");
        assert_eq!(hostname_label(""), "Rex Linux");
        assert_eq!(hostname_label(" \n "), "Rex Linux");
    }

    #[test]
    fn tailscale_opt_in_is_off_by_default_and_round_trips() {
        let old: Settings =
            serde_json::from_str(r#"{"hosts":[{"label":"a","endpoint":"unix:///run/rex.sock"}]}"#)
                .unwrap();
        assert!(!old.hosts[0].keep_tailscale);
        let saved = serde_json::to_value(&old).unwrap();
        assert!(saved["hosts"][0].get("keep_tailscale").is_none());
        assert!(saved["hosts"][0].get("tailscale_name").is_none());
        assert!(old.hosts[0].tailscale_name().unwrap().starts_with("rex"));
        assert!(old.hosts[0].controls_tailscale());
        assert_eq!(old.hosts[0].kept_tailscale(), None, "off by default");
        let mut h = old.hosts[0].clone();
        h.keep_tailscale = true;
        h.tailscale_name = Some("rex-a".into());
        assert_eq!(h.kept_tailscale().as_deref(), Some("rex-a"));
        h.endpoint = "ssh://me@box".into();
        assert!(!h.controls_tailscale());
        assert_eq!(h.kept_tailscale(), None);
    }

    #[test]
    fn an_invalid_saved_device_name_is_kept_and_stops_automatic_calls() {
        let dir = std::env::temp_dir().join(format!("rex-ts-{}", std::process::id()));
        let path = dir.join("workspace.json");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            &path,
            r#"{"hosts":[{"label":"a","endpoint":"unix:///run/rex.sock",
            "keep_tailscale":true,"tailscale_name":"bad name"}]}"#,
        )
        .unwrap();
        let s = Settings::load(&path).unwrap();
        assert!(s.hosts[0].keep_tailscale);
        assert_eq!(s.hosts[0].tailscale_name.as_deref(), Some("bad name"));
        assert!(
            s.hosts[0]
                .tailscale_name()
                .unwrap_err()
                .contains("bad name")
        );
        assert_eq!(s.hosts[0].kept_tailscale(), None);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn older_files_load_and_the_notice_corner_round_trips() {
        let old: Settings = serde_json::from_str(
            r#"{"hosts":[{"label":"a","endpoint":"unix:///run/rex.sock"}],"font_size":14}"#,
        )
        .unwrap();
        assert!(!old.hosts[0].remote_control);
        assert!(old.client_label.is_none());
        assert_eq!(old.client_label(), hostname());
        assert!(
            serde_json::to_value(&old).unwrap()["hosts"][0]
                .get("remote_control")
                .is_none()
        );
        assert_eq!(old.notice_position, NoticePosition::TopRight);
        assert_eq!(old.notifications, NotificationsConfig::default());
        assert_eq!(old.tab_layout, TabLayout::Tiled);
        assert_eq!(old.focus_centering, FocusCentering::Never);
        assert!(old.tab_layouts.is_empty());
        assert!(old.column_widths.is_empty());
        let partial: Settings =
            serde_json::from_str(r#"{"notifications":{"done":false}}"#).unwrap();
        assert!(!partial.notifications.done.desktop && partial.notifications.blocked.desktop);
        let mut s = old;
        s.notice_position = NoticePosition::BottomRight;
        s.tab_layout = TabLayout::Scrolling;
        s.focus_centering = FocusCentering::OnOverflow;
        s.tab_layouts
            .entry("unix:///run/rex.sock".into())
            .or_default()
            .entry("session:a".into())
            .or_default()
            .insert("window:a".into(), TabLayout::Tiled);
        s.column_widths
            .entry("unix:///run/rex.sock".into())
            .or_default()
            .entry("session:a".into())
            .or_default()
            .entry("window:a".into())
            .or_default()
            .insert("split:a".into(), 2. / 3.);
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(json["notice_position"], "bottom_right");
        assert_eq!(json["tab_layout"], "scrolling");
        assert_eq!(json["focus_centering"], "on_overflow");
        let back: Settings = serde_json::from_value(json).unwrap();
        assert_eq!(back.notice_position, NoticePosition::BottomRight);
        assert_eq!(back.focus_centering, FocusCentering::OnOverflow);
        assert_eq!(
            back.tab_layouts["unix:///run/rex.sock"]["session:a"]["window:a"],
            TabLayout::Tiled
        );
        assert_eq!(
            back.column_widths["unix:///run/rex.sock"]["session:a"]["window:a"]["split:a"],
            2. / 3.
        );
    }

    #[test]
    fn old_notification_switches_migrate_to_the_channels_table() {
        let c = |tab, desktop, badge, sound| Channels {
            tab,
            desktop,
            badge,
            sound,
        };
        let load = |json: &str| {
            let path = std::env::temp_dir().join(format!(
                "rex-linux-settings-notify-{}-{}.json",
                std::process::id(),
                json.len()
            ));
            std::fs::write(
                &path,
                format!(
                    r#"{{"hosts":[{{"label":"a","endpoint":"unix:///run/rex.sock"}}],
                        "notifications":{json}}}"#
                ),
            )
            .unwrap();
            let settings = Settings::load(&path).unwrap();
            std::fs::remove_file(&path).ok();
            settings.notifications
        };
        // The old defaults, written out, behave as the new defaults do.
        let old_defaults = load(
            r#"{"show_in_tabs":true,"system_notifications":true,"working":false,
                "blocked":true,"error":true,"done":true,"program_messages":true}"#,
        );
        assert_eq!(old_defaults, NotificationsConfig::default());
        // Tabs off, and desktop notifications off as a whole: every state keeps its own
        // switch's value only where notifications were on.
        let quiet = load(
            r#"{"show_in_tabs":false,"system_notifications":false,"working":true,
                "blocked":true,"error":false,"done":true,"program_messages":true}"#,
        );
        assert_eq!(quiet.working, c(false, false, false, false));
        assert_eq!(quiet.blocked, c(false, false, false, false));
        assert_eq!(quiet.error, c(false, false, false, false));
        assert_eq!(quiet.done, c(false, false, false, false));
        assert!(!quiet.messages.desktop);
        assert!(!quiet.any_tab());
        // Per-state choices under notifications that were on.
        let picky = load(
            r#"{"working":true,"blocked":false,"error":true,"done":false,
                "program_messages":false}"#,
        );
        assert_eq!(picky.working, c(true, true, false, false));
        assert_eq!(picky.blocked, c(true, false, true, false));
        assert_eq!(picky.error, c(true, true, true, false));
        assert_eq!(picky.done, c(true, false, true, false));
        assert!(!picky.messages.desktop);
        // The new shape round-trips, and the old keys are not written back.
        let mut table = NotificationsConfig::default();
        table.done.sound = true;
        table.error.badge = false;
        table.messages.sound = true;
        let json = serde_json::to_value(&table).unwrap();
        assert!(json.get("show_in_tabs").is_none() && json.get("program_messages").is_none());
        assert_eq!(json["done"]["sound"], true);
        let back: NotificationsConfig = serde_json::from_value(json).unwrap();
        assert_eq!(back, table);
    }

    #[test]
    fn loaded_column_widths_are_held_to_supported_bounds() {
        let path = std::env::temp_dir().join(format!(
            "rex-linux-settings-columns-{}.json",
            std::process::id()
        ));
        std::fs::write(
            &path,
            r#"{
                "hosts":[{"label":"a","endpoint":"unix:///run/rex.sock"}],
                "column_widths":{"unix:///run/rex.sock":{"s":{"w":{"narrow":0.01,"wide":4.0}}}}
            }"#,
        )
        .unwrap();
        let settings = Settings::load(&path).unwrap();
        let widths = &settings.column_widths["unix:///run/rex.sock"]["s"]["w"];
        assert_eq!(widths["narrow"], crate::layout::MIN_COLUMN_WIDTH);
        assert_eq!(widths["wide"], crate::layout::MAX_COLUMN_WIDTH);
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn activity_view_settings_default_and_round_trip() {
        let old: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(old.activity_grouping, ActivityGrouping::Priority);
        assert!(!old.activity_show_idle);
        assert_eq!(old.sidebar_mode, SidebarMode::Sessions);
        let saved: Settings = serde_json::from_value(serde_json::json!({
            "activity_grouping": "session",
            "activity_show_idle": true,
            "sidebar_mode": "activity",
        }))
        .unwrap();
        assert_eq!(saved.activity_grouping, ActivityGrouping::Session);
        assert!(saved.activity_show_idle);
        assert_eq!(saved.sidebar_mode, SidebarMode::Activity);
    }

    #[test]
    fn the_session_list_defaults_to_by_host_and_round_trips() {
        let old: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(old.sidebar_sessions, SessionList::ByHost);
        let saved: Settings =
            serde_json::from_value(serde_json::json!({ "sidebar_sessions": "flat" })).unwrap();
        assert_eq!(saved.sidebar_sessions, SessionList::Flat);
        let json = serde_json::to_value(&saved).unwrap();
        assert_eq!(json["sidebar_sessions"], "flat");
    }

    #[test]
    fn generated_titles_default_off_for_old_files_and_round_trip() {
        let old: Settings =
            serde_json::from_str(r#"{"font_size":17,"vertical_tabs":true}"#).unwrap();
        assert!(!old.generated_titles);
        assert_eq!(old.font_size, 17.);
        assert!(old.vertical_tabs);
        for generated_titles in [true, false] {
            let settings = Settings {
                generated_titles,
                ..Settings::default()
            };
            let saved = serde_json::to_value(&settings).unwrap();
            assert_eq!(saved["generated_titles"], generated_titles);
            let back: Settings = serde_json::from_value(saved).unwrap();
            assert_eq!(back.generated_titles, generated_titles);
        }
    }

    #[test]
    fn the_terminal_theme_round_trips_and_unknown_values_follow_the_server() {
        let old: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(old.terminal_theme, TerminalThemeChoice::FollowServer);
        assert!(!old.terminal_theme_declared && !old.terminal_theme_chosen);
        for (choice, saved) in [
            (TerminalThemeChoice::FollowServer, "follow_server"),
            (TerminalThemeChoice::RexDefault, "builtin_default"),
            (TerminalThemeChoice::FollowChrome, "follow_chrome"),
            (
                TerminalThemeChoice::Named("rex/midnight".into()),
                "named:rex/midnight",
            ),
            (
                TerminalThemeChoice::Named("Catppuccin Mocha".into()),
                "named:Catppuccin Mocha",
            ),
        ] {
            let s = Settings {
                terminal_theme: choice.clone(),
                ..Settings::default()
            };
            let json = serde_json::to_value(&s).unwrap();
            assert_eq!(json["terminal_theme"], saved);
            let back: Settings = serde_json::from_value(json).unwrap();
            assert_eq!(back.terminal_theme, choice);
        }
        for unknown in ["follow_wallpaper", "named:", "Catppuccin Mocha"] {
            let s: Settings =
                serde_json::from_value(serde_json::json!({ "terminal_theme": unknown })).unwrap();
            assert_eq!(
                s.terminal_theme,
                TerminalThemeChoice::FollowServer,
                "{unknown}"
            );
        }
    }

    #[test]
    fn pane_density_defaults_to_comfortable_and_round_trips() {
        let old: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(old.pane_density, PaneDensity::Comfortable);
        for (density, saved) in [
            (PaneDensity::Compact, "compact"),
            (PaneDensity::Comfortable, "comfortable"),
        ] {
            let settings = Settings {
                pane_density: density,
                ..Settings::default()
            };
            let json = serde_json::to_value(&settings).unwrap();
            assert_eq!(json["pane_density"], saved);
            let back: Settings = serde_json::from_value(json).unwrap();
            assert_eq!(back.pane_density, density);
        }
    }

    #[test]
    fn the_downloads_directory_defaults_to_none_and_round_trips() {
        let old: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(old.downloads.directory, None);
        assert!(
            serde_json::to_value(&old).unwrap()["downloads"]
                .get("directory")
                .is_none(),
            "an unset folder is not written"
        );
        let path = std::env::temp_dir().join(format!(
            "rex-downloads-settings-{}.json",
            std::process::id()
        ));
        for directory in [Some("~/Downloads/rex"), Some("/srv/files"), None] {
            let mut settings = Settings::default();
            settings.downloads.directory = directory.map(str::to_owned);
            settings.save(&path).unwrap();
            let saved = std::fs::read_to_string(&path).unwrap();
            assert_eq!(saved.contains("\"directory\""), directory.is_some());
            assert_eq!(
                Settings::load(&path)
                    .unwrap()
                    .downloads
                    .directory
                    .as_deref(),
                directory
            );
        }
        std::fs::write(&path, r#"{"downloads":{"directory":"/a/b"}}"#).unwrap();
        assert_eq!(
            Settings::load(&path)
                .unwrap()
                .downloads
                .directory
                .as_deref(),
            Some("/a/b"),
            "the key is downloads.directory"
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn pane_header_defaults_to_auto_round_trips_and_reads_an_unknown_value_as_auto() {
        let old: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(old.pane_header, PaneHeader::Auto);
        for (header, saved) in [
            (PaneHeader::Auto, "auto"),
            (PaneHeader::Always, "always"),
            (PaneHeader::OnHover, "on_hover"),
            (PaneHeader::Hidden, "hidden"),
        ] {
            let settings = Settings {
                pane_header: header,
                ..Settings::default()
            };
            let json = serde_json::to_value(&settings).unwrap();
            assert_eq!(json["pane_header"], saved);
            let back: Settings = serde_json::from_value(json).unwrap();
            assert_eq!(back.pane_header, header);
        }
        let newer: Settings =
            serde_json::from_value(serde_json::json!({ "pane_header": "on_focus" })).unwrap();
        assert_eq!(newer.pane_header, PaneHeader::Auto);
    }

    #[test]
    fn updates_check_defaults_to_true_and_round_trips() {
        let old: Settings = serde_json::from_str("{}").unwrap();
        assert!(old.updates.check);
        for check in [false, true] {
            let settings = Settings {
                updates: UpdatesConfig { check },
                ..Settings::default()
            };
            let json = serde_json::to_value(&settings).unwrap();
            assert_eq!(json["updates"]["check"], check);
            let back: Settings = serde_json::from_value(json).unwrap();
            assert_eq!(back.updates.check, check);
        }
    }

    #[test]
    fn the_chrome_theme_round_trips_and_unknown_values_draw_as_midnight() {
        let old: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(old.chrome_theme, ChromeTheme::Midnight);
        for (chrome, saved) in [
            (ChromeTheme::Midnight, "midnight"),
            (ChromeTheme::RosePine, "rose_pine"),
            (ChromeTheme::RosePineMoon, "rose_pine_moon"),
            (ChromeTheme::GruvboxDark, "gruvbox_dark"),
        ] {
            let s = Settings {
                chrome_theme: chrome,
                terminal_theme_chosen: true,
                ..Settings::default()
            };
            let json = serde_json::to_value(&s).unwrap();
            assert_eq!(json["chrome_theme"], saved);
            let back: Settings = serde_json::from_value(json).unwrap();
            assert_eq!(back.chrome_theme, chrome);
            assert!(back.terminal_theme_chosen);
        }
        let s: Settings =
            serde_json::from_value(serde_json::json!({ "chrome_theme": "solarized_light" }))
                .unwrap();
        assert_eq!(s.chrome_theme, ChromeTheme::Unavailable("solarized_light"));
        assert_eq!(s.chrome_theme.effective(), ChromeTheme::Midnight);
        assert_eq!(s.chrome_theme.name(), "Midnight");
    }

    #[test]
    fn an_unknown_chrome_survives_a_save() {
        let path =
            std::env::temp_dir().join(format!("rex-unknown-chrome-{}.json", std::process::id()));
        std::fs::write(&path, r#"{"chrome_theme":"not_a_real_chrome"}"#).unwrap();
        let loaded = Settings::load(&path).unwrap();
        assert_eq!(
            loaded.chrome_theme,
            ChromeTheme::Unavailable("not_a_real_chrome")
        );
        loaded.save(&path).unwrap();
        let json: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(json["chrome_theme"], "not_a_real_chrome");
    }

    #[test]
    fn generated_chromes_round_trip() {
        let all = ChromeTheme::all();
        // The shipped four plus every generated theme with a chrome (six under the fixture).
        let generated = crate::dracula_pro::THEMES
            .iter()
            .filter(|t| t.chrome.is_some())
            .count();
        assert_eq!(all.len(), 4 + generated);
        for chrome in all {
            let json = serde_json::to_value(chrome).unwrap();
            let back: ChromeTheme = serde_json::from_value(json).unwrap();
            assert_eq!(back, chrome);
            if let ChromeTheme::Generated(i) = chrome {
                assert_eq!(chrome.name(), crate::dracula_pro::THEMES[i as usize].name);
                assert_eq!(chrome.effective(), chrome);
            }
        }
    }

    #[test]
    fn hosts_take_https_ssh_and_unix_endpoints() {
        for ok in [
            "https://box.tail1234.ts.net",
            "http://box:8080",
            "ssh://me@box.tail1234.ts.net",
            "ssh://box:2222",
            "ssh://ops@box/run/rex/server.sock",
            "unix:///run/user/1000/rex/server.sock",
        ] {
            validate_host("box", ok).unwrap_or_else(|e| panic!("{ok}: {e}"));
        }
        for (bad, words) in [
            ("ftp://box", "https://, ssh:// or unix://"),
            ("box", "https://, ssh:// or unix://"),
            ("ssh://", "ssh://user@host"),
            ("ssh://box/", "absolute socket path"),
            ("ssh://box/a b.sock", "ssh://user@host"),
            ("unix://relative.sock", "absolute"),
        ] {
            let error = validate_host("box", bad).unwrap_err().to_string();
            assert!(error.contains(words), "{bad}: {error}");
        }
        assert!(validate_host("  ", "https://box").is_err());
        assert_eq!(
            endpoint_label("ssh://me@box.tail1234.ts.net"),
            "box.tail1234.ts.net"
        );
        assert_eq!(
            endpoint_label("https://box.tail1234.ts.net:8443"),
            "box.tail1234.ts.net"
        );
        assert_eq!(
            endpoint_label("unix:///tmp/rex.sock"),
            "unix:///tmp/rex.sock"
        );
    }
}
