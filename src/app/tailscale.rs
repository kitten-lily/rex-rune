//! A local host's built-in Tailscale in Settings → Hosts: on and off, its device name, and
//! keeping it on after restarts. Every call here is an explicit user action.
use super::{
    panel::{Form, PanelKind},
    settings_kit::{row_frame, setting},
    settings_view::{detail_text, setting_with},
    theme::*,
    widgets::{Pill, pill},
    workspace::Workspace,
};
use rex_linux::connection::{
    HostCommand,
    tailscale::{Call, Status},
};
use rune::{Color, El, Shape, canvas, col, row, text};

/// The Tailscale rows of saved host `host`'s card: none unless it is local and its server can
/// run built-in Tailscale.
pub(super) fn rows(s: &Workspace, host: usize) -> Vec<El> {
    let saved = &s.settings.hosts[host];
    let Some(model) = s.hosts.get(host).map(|h| &h.model) else {
        return Vec::new();
    };
    let pairing = model
        .tailscale
        .as_ref()
        .filter(|_| model.connected)
        .and_then(pairing_url)
        .map(|_| {
            setting(
                "Pairing code",
                "Pair Rex on a phone or tablet on this tailnet.",
                pill(
                    &format!("tailscale-pair:{host}"),
                    "Show…",
                    Pill::Neutral,
                    26.,
                    move |s, _| s.show_pairing(host),
                ),
            )
        });
    let (Some(status), true) = (model.tailscale.as_ref(), saved.controls_tailscale()) else {
        return pairing.into_iter().collect();
    };
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
    let mut rows = vec![setting_with(
        "Tailscale",
        Some(detail_text(&words, if warn { warning() } else { muted() })),
        control,
    )];
    if let Some(problem) = &model.tailscale_problem {
        rows.push(
            row_frame(false).child(
                text(problem.clone())
                    .font(ui(11.5))
                    .color(rgb(warning()))
                    .wrap()
                    .grow(1.)
                    .min_w(0.)
                    .key(format!("tailscale-problem:{host}")),
            ),
        );
    }
    let name = match saved.tailscale_name() {
        Ok(name) => detail_text(&name, muted()),
        Err(problem) => {
            detail_text(&problem, warning()).key(format!("tailscale-name-problem:{host}"))
        }
    };
    rows.push(setting_with(
        "Device name",
        Some(name),
        pill(
            &format!("tailscale-name:{host}"),
            "Edit…",
            Pill::Neutral,
            26.,
            move |s, _| s.open_tailscale_name(host),
        ),
    ));
    rows.push(setting(
        "Keep on after restarts",
        "Turn Tailscale on under this name now and whenever Rex Linux connects to this server.",
        rune_kit::Switch::new(saved.keep_tailscale)
            .label(format!("Keep Tailscale on for {}", saved.label))
            .hide_label()
            .on_toggle(move |s: &mut Workspace| s.toggle_keep_tailscale(host))
            .build(kit())
            .key(format!("tailscale-keep:{host}")),
    ));
    rows.extend(pairing);
    rows
}

/// The address a device on the tailnet reaches this server at, while HTTPS is available.
pub(super) fn pairing_url(status: &Status) -> Option<&str> {
    status
        .https
        .as_ref()
        .filter(|https| https.state == "available")
        .and_then(|https| https.url.as_deref())
}

/// The QR code for `url`: its width in modules and its dark modules, row by row.
pub(super) fn qr_modules(url: &str) -> Option<(usize, Vec<bool>)> {
    let code = qrcode::QrCode::new(url.as_bytes()).ok()?;
    let dark = code
        .to_colors()
        .into_iter()
        .map(|c| c == qrcode::Color::Dark)
        .collect();
    Some((code.width(), dark))
}

/// Pixels per module of the pairing code.
const MODULE: f32 = 6.;
/// Light modules around the code that scanners need.
const QUIET: usize = 4;

/// The code as one canvas of squares on a light ground, so it scans in dark chrome.
fn qr_canvas(url: &str) -> Option<El> {
    let (w, dark) = qr_modules(url)?;
    let side = ((w + 2 * QUIET) as f32) * MODULE;
    let square = |x0: f32, y0: f32, x1: f32, y1: f32| [[x0, y0], [x1, y0], [x1, y1], [x0, y1]];
    let mut shape = Shape::new().fill(square(0., 0., side, side), Color::WHITE);
    for y in 0..w {
        // One square per run of dark modules in the row.
        let mut x = 0;
        while x < w {
            if !dark[y * w + x] {
                x += 1;
                continue;
            }
            let start = x;
            while x < w && dark[y * w + x] {
                x += 1;
            }
            let top = (y + QUIET) as f32 * MODULE;
            shape = shape.fill(
                square(
                    (start + QUIET) as f32 * MODULE,
                    top,
                    (x + QUIET) as f32 * MODULE,
                    top + MODULE,
                ),
                Color::BLACK,
            );
        }
    }
    Some(canvas(shape).size(side, side).key("pairing-qr"))
}

/// The "Pair a device" dialog for the saved host at `endpoint`, from its status right now.
pub(super) fn pairing(s: &Workspace, endpoint: &str) -> El {
    let panel = s.panel.as_ref().expect("pairing panel");
    let url = s
        .settings
        .hosts
        .iter()
        .position(|h| h.endpoint == endpoint)
        .and_then(|i| s.hosts.get(i))
        .map(|h| &h.model)
        .filter(|m| m.connected)
        .and_then(|m| m.tailscale.as_ref())
        .and_then(pairing_url);
    let mut body = col().gap(12.).child(
        text(panel.title.clone())
            .font(strong(17.))
            .color(rgb(ink())),
    );
    match url.and_then(|url| qr_canvas(url).map(|qr| (url, qr))) {
        Some((url, qr)) => {
            body = body
                .child(row().justify_center().child(qr))
                .child(
                    text(url.to_owned())
                        .font(ui(12.).mono())
                        .color(rgb(ink()))
                        .wrap()
                        .selectable(true)
                        .key("pairing-url"),
                )
                .child(
                    text(
                        "In Rex on your other device, tap Pair via QR Code. That device must be on the same tailnet.",
                    )
                    .font(ui(12.))
                    .color(rgb(muted()))
                    .wrap(),
                );
        }
        None => {
            body = body.child(
                text("This server isn't reachable over the tailnet right now.")
                    .font(ui(12.))
                    .color(rgb(muted()))
                    .wrap()
                    .key("pairing-unavailable"),
            );
        }
    }
    body.child(
        row().justify_end().child(
            pill("pairing-close", "Close", Pill::Neutral, 32., |s, tx| {
                s.cancel_panel(tx)
            })
            .tab_index(1),
        ),
    )
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

    /// Opens the pairing code for a connected host whose HTTPS address is available.
    pub fn show_pairing(&mut self, host: usize) {
        let endpoint = self.settings.hosts[host].endpoint.clone();
        self.open_panel(PanelKind::Pairing { endpoint }, "Pair a device", &[]);
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
