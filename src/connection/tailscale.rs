//! Keeping a host's built-in Tailscale on and named: device naming rules, and the reconciler that
//! decides when the client enables Tailscale or renames the device.
use serde_json::Value;

pub type Status = rexrs::api::session::Tailscale;

/// The problem shown for a call to a server without built-in Tailscale.
pub const UNSUPPORTED: &str = "This server can't run built-in Tailscale.";

/// A Tailscale call the client should make.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Call {
    Enable { hostname: String },
    Disable,
    Rename { hostname: String },
}

impl Call {
    /// The call as a verb phrase, for problem text.
    pub fn verb(&self) -> &'static str {
        match self {
            Self::Enable { .. } => "turn on Tailscale",
            Self::Disable => "turn off Tailscale",
            Self::Rename { .. } => "rename the Tailscale device",
        }
    }
}

/// `rex-<first label of hostname>`, lowercased, as a valid device name.
pub fn default_name(hostname: &str) -> String {
    let label = hostname
        .split('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    let name: String = format!("rex-{label}")
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let name = name.trim_matches('-');
    let name = name
        .get(..name.len().min(63))
        .unwrap_or(name)
        .trim_matches('-');
    if name.is_empty() {
        "rex".into()
    } else {
        name.into()
    }
}

/// 1-63 ASCII letters, digits and hyphens, not starting or ending with a hyphen.
pub fn valid_name(name: &str) -> bool {
    (1..=63).contains(&name.len())
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        && !name.starts_with('-')
        && !name.ends_with('-')
}

/// Whether a MagicDNS name's first label is `wanted` or `<wanted>-<digits>`, ignoring case.
pub fn name_matches(dns_name: &str, wanted: &str) -> bool {
    let label = dns_name
        .split('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    let wanted = wanted.to_ascii_lowercase();
    if label == wanted {
        return true;
    }
    label
        .strip_prefix(&format!("{wanted}-"))
        .is_some_and(|rest| !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()))
}

/// Converts a call reply's status to the event/bootstrap type (the two are field-for-field twins).
pub fn from_server(status: rexrs::api::server::Tailscale) -> Status {
    serde_json::to_value(status)
        .and_then(serde_json::from_value)
        .unwrap_or_default()
}

/// The status in a `tailscale_status_changed` event's payload, which is the status object itself.
pub fn parse_event(payload: &Value) -> Option<Status> {
    serde_json::from_value(payload.clone()).ok()
}

/// What one connection has already done to a host's Tailscale.
#[derive(Default)]
pub struct Keeper {
    enabled: bool,
    renamed: Option<String>,
    awaiting: bool,
}

impl Keeper {
    pub fn at_connect(&mut self, status: &Status, wanted: &str) -> Option<Call> {
        self.next(status, wanted, true)
    }

    pub fn on_event(&mut self, status: &Status, wanted: &str) -> Option<Call> {
        self.awaiting = false;
        self.next(status, wanted, false)
    }

    fn next(&mut self, status: &Status, wanted: &str, connect: bool) -> Option<Call> {
        match status.state.as_str() {
            "disabled" if connect && status.error.is_none() && !self.enabled => {
                self.enabled = true;
                Some(Call::Enable {
                    hostname: wanted.into(),
                })
            }
            "running" => {
                if self.awaiting
                    || status
                        .dns_name
                        .as_deref()
                        .is_none_or(|dns| name_matches(dns, wanted))
                    || self.renamed.as_deref() == Some(wanted)
                {
                    return None;
                }
                self.renamed = Some(wanted.into());
                self.awaiting = true;
                Some(Call::Rename {
                    hostname: wanted.into(),
                })
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The payload of the first `tailscale_status_changed` event, recorded by
    /// `scripts/tailscale-event-contract.py`.
    const RECORDED: &str =
        r#"{"state":"starting","allow_tailnet_members":false,"https":{"state":"pending"}}"#;

    fn status(state: &str, dns: Option<&str>, error: Option<&str>) -> Status {
        serde_json::from_value(json!({"state":state,"dns_name":dns,"error":error,
            "allow_tailnet_members":true}))
        .unwrap()
    }

    #[test]
    fn default_names_are_valid_device_names() {
        assert_eq!(default_name("Studio.local"), "rex-studio");
        assert_eq!(default_name("my_box"), "rex-my-box");
        assert_eq!(default_name("--"), "rex");
        assert_eq!(default_name(""), "rex");
        let long = default_name(&"a".repeat(80));
        assert_eq!(long.len(), 63);
        assert!(valid_name(&long));
        assert!(valid_name("rex-studio") && valid_name("a"));
        for bad in [
            "",
            "-rex",
            "rex-",
            "rex studio",
            "rex.studio",
            &"a".repeat(64),
            "rëx",
        ] {
            assert!(!valid_name(bad), "{bad:?}");
        }
    }

    #[test]
    fn magicdns_suffixes_still_match() {
        assert!(name_matches("rex-studio.tail1234.ts.net", "rex-studio"));
        assert!(name_matches("REX-Studio-1.tail1234.ts.net", "rex-studio"));
        assert!(name_matches("rex-studio-12.tail1234.ts.net.", "rex-studio"));
        assert!(!name_matches("rex.tail1234.ts.net", "rex-studio"));
        assert!(!name_matches("rex-studio-x.tail1234.ts.net", "rex-studio"));
        assert!(!name_matches("rex-studio-.tail1234.ts.net", "rex-studio"));
    }

    #[test]
    fn reply_statuses_convert_without_loss() {
        let wire = json!({"state":"running","dns_name":"rex-a.t.ts.net","allow_tailnet_members":false,
            "https":{"state":"available","url":"https://rex-a.t.ts.net"}});
        let server: rexrs::api::server::Tailscale = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(from_server(server)).unwrap(), wire);
    }

    #[test]
    fn the_keeper_enables_once_at_connect_and_never_after_a_listener_failure() {
        let mut k = Keeper::default();
        assert_eq!(k.on_event(&status("disabled", None, None), "rex-a"), None);
        assert_eq!(
            k.at_connect(&status("disabled", None, None), "rex-a"),
            Some(Call::Enable {
                hostname: "rex-a".into()
            })
        );
        assert_eq!(k.at_connect(&status("disabled", None, None), "rex-a"), None);
        let mut failed = Keeper::default();
        assert_eq!(
            failed.at_connect(
                &status("disabled", None, Some("listen: address in use")),
                "rex-a"
            ),
            None
        );
        for state in [
            "starting",
            "needs_login",
            "needs_machine_approval",
            "mystery",
        ] {
            assert_eq!(
                Keeper::default().at_connect(&status(state, Some("rex.t.ts.net"), None), "rex-a"),
                None
            );
        }
    }

    #[test]
    fn the_keeper_renames_once_and_waits_for_an_event() {
        let mut k = Keeper::default();
        let old = status("running", Some("rex.t.ts.net"), None);
        assert_eq!(
            k.at_connect(&old, "rex-a"),
            Some(Call::Rename {
                hostname: "rex-a".into()
            })
        );
        assert_eq!(k.at_connect(&old, "rex-a"), None, "awaiting the event");
        assert_eq!(
            k.on_event(&old, "rex-a"),
            None,
            "one rename per wanted name"
        );
        assert_eq!(
            k.on_event(&status("running", Some("rex-a-1.t.ts.net"), None), "rex-a"),
            None
        );
        assert_eq!(
            k.on_event(&old, "rex-b"),
            Some(Call::Rename {
                hostname: "rex-b".into()
            })
        );
        assert_eq!(
            Keeper::default().at_connect(&status("running", None, None), "rex-a"),
            None
        );
    }

    #[test]
    fn events_parse_the_recorded_shape() {
        let recorded: serde_json::Value = serde_json::from_str(RECORDED).unwrap();
        let parsed = parse_event(&recorded).unwrap();
        assert_eq!(parsed.state, "starting");
        assert!(parse_event(&json!({"nope":1})).is_none());
    }
}
