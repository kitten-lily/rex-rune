//! A local host's built-in Tailscale in Settings → Hosts: on and off, its device name, and
//! keeping it on after restarts. Every call here is an explicit user action.
use super::{
    panel::{Form, PanelKind},
    settings_view::{detail_text, setting, setting_with},
    theme::*,
    widgets::{Pill, pill},
    workspace::Workspace,
};
use rex_linux::connection::{
    HostCommand,
    tailscale::{Call, Status},
};
use rune::{El, col, row, text};

/// The Tailscale rows for saved host `host`: empty unless it is local and its server can run
/// built-in Tailscale.
pub(super) fn rows(s: &Workspace, host: usize) -> El {
    let saved = &s.settings.hosts[host];
    let status = s.hosts.get(host).and_then(|h| h.model.tailscale.as_ref());
    let (Some(status), true) = (status, saved.controls_tailscale()) else {
        return col();
    };
    let model = &s.hosts[host].model;
    let (words, warn) = status_words(status);
    let mut control = row().gap(8.).center_y();
    if status.state == "needs_login"
        && let Some(url) = status.auth_url.clone()
    {
        control = control.child(pill(
            &format!("tailscale-sign-in:{host}"),
            "Sign in…",
            Pill::Neutral,
            26.,
            move |s, _| s.open_link(url.clone()),
        ));
    }
    control = control.child(
        rune_kit::Switch::new(status.state != "disabled")
            .label(format!("Tailscale for {}", saved.label))
            .hide_label()
            .on_toggle(move |s: &mut Workspace| s.toggle_tailscale(host))
            .build(kit())
            .key(format!("tailscale:{host}")),
    );
    let mut body = col().gap(4.).child(setting_with(
        "Tailscale",
        Some(detail_text(&words, if warn { warning() } else { muted() })),
        control,
    ));
    if let Some(problem) = &model.tailscale_problem {
        body = body.child(
            text(problem.clone())
                .font(ui(11.5))
                .color(rgb(warning()))
                .wrap()
                .key(format!("tailscale-problem:{host}")),
        );
    }
    let name = match saved.tailscale_name() {
        Ok(name) => detail_text(&name, muted()),
        Err(problem) => {
            detail_text(&problem, warning()).key(format!("tailscale-name-problem:{host}"))
        }
    };
    body.child(setting_with(
        "Device name",
        Some(name),
        pill(
            &format!("tailscale-name:{host}"),
            "Edit…",
            Pill::Neutral,
            26.,
            move |s, _| s.open_tailscale_name(host),
        ),
    ))
    .child(setting(
        "Keep on after restarts",
        "Turn Tailscale back on under this name whenever Rex Linux connects to this server.",
        rune_kit::Switch::new(saved.keep_tailscale)
            .label(format!("Keep Tailscale on for {}", saved.label))
            .hide_label()
            .on_toggle(move |s: &mut Workspace| s.toggle_keep_tailscale(host))
            .build(kit())
            .key(format!("tailscale-keep:{host}")),
    ))
}

/// The status as words for the Tailscale row, and whether they describe a problem.
pub(super) fn status_words(status: &Status) -> (String, bool) {
    match status.state.as_str() {
        "disabled" => match &status.error {
            Some(error) => (format!("Off · {error}"), true),
            None => ("Off".into(), false),
        },
        "needs_login" => ("Needs sign-in".into(), false),
        "needs_machine_approval" => (
            "Waiting for a tailnet admin to approve this device".into(),
            true,
        ),
        "running" => {
            let mut words = match &status.dns_name {
                Some(dns) => format!("On as {dns}"),
                None => "On".into(),
            };
            if status
                .https
                .as_ref()
                .is_some_and(|h| h.state != "available")
            {
                words.push_str(" · HTTPS starting…");
            }
            (words, false)
        }
        _ => ("Starting…".into(), false),
    }
}

impl Workspace {
    /// The Tailscale switch: on enables under the device name; off also stops keeping it on.
    pub fn toggle_tailscale(&mut self, host: usize) {
        let off = self.hosts[host]
            .model
            .tailscale
            .as_ref()
            .is_none_or(|t| t.state == "disabled");
        if off {
            // An invalid saved name is already shown as a problem on the name row.
            if let Ok(hostname) = self.settings.hosts[host].tailscale_name() {
                self.send(host, HostCommand::Tailscale(Call::Enable { hostname }));
            }
            return;
        }
        if self.settings.hosts[host].keep_tailscale {
            self.settings.hosts[host].keep_tailscale = false;
            self.save();
            self.send(host, HostCommand::KeepTailscale(None));
        }
        self.send(host, HostCommand::Tailscale(Call::Disable));
    }

    /// Keep on after restarts; the worker enables or renames now if needed.
    pub fn toggle_keep_tailscale(&mut self, host: usize) {
        let saved = &mut self.settings.hosts[host];
        saved.keep_tailscale = !saved.keep_tailscale;
        let kept = saved.kept_tailscale();
        self.save();
        self.send(host, HostCommand::KeepTailscale(kept));
    }

    /// Saves a valid device name and applies it: through the keeper when Keep is on, else by
    /// renaming a running device.
    pub fn set_tailscale_name(&mut self, host: usize, name: String) {
        self.settings.hosts[host].tailscale_name = Some(name.clone());
        self.save();
        if let Some(kept) = self.settings.hosts[host].kept_tailscale() {
            self.send(host, HostCommand::KeepTailscale(Some(kept)));
        } else if self.hosts[host]
            .model
            .tailscale
            .as_ref()
            .is_some_and(|t| t.state == "running")
        {
            self.send(
                host,
                HostCommand::Tailscale(Call::Rename { hostname: name }),
            );
        }
    }

    /// The device name form, prefilled with the saved name (valid or not), else the default.
    pub(super) fn open_tailscale_name(&mut self, host: usize) {
        let saved = &self.settings.hosts[host];
        let title = format!("Tailscale device name on {}", saved.label);
        let name = saved
            .tailscale_name
            .clone()
            .or_else(|| saved.tailscale_name().ok())
            .unwrap_or_default();
        self.open_panel(
            PanelKind::Form(Form::TailscaleName(host)),
            &title,
            &[("Device name", "1–63 letters, digits and hyphens", &name)],
        );
    }
}
