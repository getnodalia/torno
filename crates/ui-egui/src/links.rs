//! Support and community links: the Donate and Discord pills in the title bar before Report a bug, the
//! Help menu entries, the About window's buttons and the start page's links all come from [`LINKS`],
//! so a URL or a label is written once. Clicking one opens its page in the system browser; nothing
//! here opens a page by itself.
//!
//! The title bar's pills follow two preferences (Preferences › General › Community, both on by
//! default). The small × inside a pill turns its preference off. The Help menu, About and the start
//! page keep their links whatever those preferences say.

use crate::SolveApp;
use crate::prefs::Prefs;

/// One link and every place that shows it.
pub struct Link {
    /// The id the Help menu and the start page's links go by.
    pub id: &'static str,
    pub url: &'static str,
    /// The icon master (`crates/look/icons/design/svg/flat`): the service's brand mark.
    pub icon: &'static str,
    /// Message ids: the Help menu entry, the status bar button's short text, the longer text of the
    /// About and start page buttons, the tooltip and the status bar's hide button tooltip.
    pub menu: &'static str,
    pub button: &'static str,
    pub label: &'static str,
    pub tip: &'static str,
    pub hide_tip: &'static str,
    /// Message ids of the accessible names of the pill and of its ×.
    pub name: &'static str,
    pub hide_name: &'static str,
    /// Whether the title bar shows this link's pill, whether it carries its name on a wide bar, and
    /// the switch that stops it.
    pub named: bool,
    pub fill: egui::Color32,
    pub shown: fn(&Prefs) -> bool,
    pub hide: fn(&mut Prefs),
}

/// Torno ships no community or donation links: upstream's point at its own Ko-fi and Discord.
/// The pills, menu entries and buttons are drawn from this list, so an empty one shows none.
pub const LINKS: &[Link] = &[];

pub fn by_id(id: &str) -> Option<&'static Link> {
    LINKS.iter().find(|l| l.id == id)
}

/// Open `url` in the system browser, as the Help menu's Documentation does.
pub fn open(ctx: &egui::Context, url: &str) {
    ctx.open_url(egui::OpenUrl::new_tab(url));
}

/// Open the link `id`; false for an id that is not in [`LINKS`].
pub fn run(ctx: &egui::Context, id: &str) -> bool {
    by_id(id).map(|l| open(ctx, l.url)).is_some()
}

/// The links whose title bar pill is on.
pub fn title_bar(app: &SolveApp) -> impl Iterator<Item = &'static Link> + '_ {
    LINKS.iter().filter(|l| (l.shown)(&app.preferences))
}

/// The title bar's pills, left to right; `wide` gives the named ones their name.
pub fn pills(app: &SolveApp, wide: bool) -> Vec<torno_ui_kit::app_bar::Pill<'static>> {
    title_bar(app)
        .map(|l| torno_ui_kit::app_bar::Pill {
            id: l.id,
            icon: l.icon,
            label: if l.named { torno_ui_text::tr(l.button) } else { "" },
            labelled: l.named && wide,
            tip: torno_ui_text::tr(l.tip),
            hide_tip: torno_ui_text::tr(l.hide_tip),
            name: torno_ui_text::tr(l.name),
            hide_name: torno_ui_text::tr(l.hide_name),
            fill: l.fill,
            ink: egui::Color32::WHITE,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn torno_ships_no_community_links() {
        assert!(LINKS.is_empty());
        let ctx = egui::Context::default();
        ctx.begin_pass(egui::RawInput::default());
        assert!(!run(&ctx, "kofi"));
        assert!(!run(&ctx, "discord"));
        let mut out = ctx.end_pass();
        out.textures_delta.clear();
        assert!(out.platform_output.commands.iter().all(|c| !matches!(c, egui::OutputCommand::OpenUrl(_))));
    }

    #[test]
    fn every_link_has_text_and_an_icon_in_the_catalog() {
        crate::i18n::install();
        for link in LINKS {
            for id in [link.menu, link.button, link.label, link.tip, link.hide_tip, link.name, link.hide_name] {
                assert_ne!(torno_ui_text::tr(id), id, "{id} has no English text");
            }
            assert!(torno_look::icons::SET.has_master(link.icon), "{}", link.icon);
        }
    }

    #[test]
    fn the_title_bar_shows_no_pills() {
        let app = SolveApp::new(Default::default(), Default::default());
        assert_eq!(title_bar(&app).count(), 0);
    }
}
