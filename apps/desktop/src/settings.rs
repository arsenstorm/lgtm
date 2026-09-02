//! Settings: a full window of its own, the way Codex does it. How the app
//! looks, when it speaks up, what a task is run with, and where the
//! orchestrator it talks to lives.

use crate::app::LgtmApp;
use crate::theme::{
    icon, section_label, tokens, Pick, Pref, TabularNums as _, Tokens, BAR_H, ICON, MONO_FONT,
    RADIUS, ROW_RADIUS, SPACE, TEXT_MONO, TEXT_ROW, TEXT_SECONDARY,
};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    deferred, div, px, AnyElement, ClickEvent, ClipboardItem, Context, Div, FontWeight,
    InteractiveElement as _, IntoElement, ParentElement as _, SharedString, Stateful,
    StatefulInteractiveElement as _, Styled as _,
};
use gpui_component::button::Button;
use gpui_component::input::Input;
use gpui_component::switch::Switch;
use gpui_component::Sizable as _;
use lgtm_diff::DiffStyle;

/// The rail, at the width of the one it replaces.
const RAIL_W: f32 = 240.;
/// The reading column. Wider than this and a row's description outruns the
/// eye's return path.
const COLUMN: f32 = 640.;
/// The page name at the top of a pane.
const TITLE: f32 = 22.;
/// The app name over its version, in About.
const MARK: f32 = 44.;
const NAV_ROW_H: f32 = 32.;
/// One dropdown, field or segment: the height every control shares.
const CONTROL_H: f32 = 28.;
const CONTROL_W: f32 = 160.;

/// One page of the settings window. The rail picks it; the pane shows it.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub enum Section {
    #[default]
    Appearance,
    Notifications,
    Models,
    Orchestrator,
    About,
}

impl Section {
    const ALL: [Section; 5] = [
        Section::Appearance,
        Section::Notifications,
        Section::Models,
        Section::Orchestrator,
        Section::About,
    ];

    fn label(self) -> &'static str {
        match self {
            Section::Appearance => "Appearance",
            Section::Notifications => "Notifications",
            Section::Models => "Models",
            Section::Orchestrator => "Orchestrator",
            Section::About => "About",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Section::Appearance => "palette",
            Section::Notifications => "bell",
            Section::Models => "cpu",
            Section::Orchestrator => "server",
            Section::About => "info",
        }
    }

    fn id(self) -> &'static str {
        match self {
            Section::Appearance => "section-appearance",
            Section::Notifications => "section-notifications",
            Section::Models => "section-models",
            Section::Orchestrator => "section-orchestrator",
            Section::About => "section-about",
        }
    }
}

/// `lgtm runner` takes a ws(s) URL; the app holds the http(s) one.
fn ws_url(orchestrator: &str) -> String {
    match orchestrator.trim_end_matches('/').split_once("://") {
        Some(("http", rest)) => format!("ws://{rest}"),
        Some(("https", rest)) => format!("wss://{rest}"),
        _ => orchestrator.trim_end_matches('/').to_string(),
    }
}

/// The line a person pastes on the machine they want to add.
pub fn join_line(orchestrator: &str, token: &str) -> String {
    format!("lgtm runner {} --token {token}", ws_url(orchestrator))
}

pub fn view(app: &LgtmApp, cx: &mut Context<LgtmApp>) -> AnyElement {
    let t = tokens(cx);
    div()
        .id("settings")
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .occlude()
        .bg(t.bg)
        .flex()
        .child(rail(app, &t, cx))
        .child(pane(app, &t, cx))
        .into_any_element()
}

fn rail(app: &LgtmApp, t: &Tokens, cx: &mut Context<LgtmApp>) -> Div {
    div()
        .flex()
        .flex_col()
        .flex_none()
        .w(px(RAIL_W))
        .gap(px(2.))
        .p(px(SPACE[1]))
        // Clear of the traffic lights, which AppKit draws over this window.
        .pt(px(BAR_H))
        .bg(t.sidebar)
        .border_r_1()
        .border_color(t.sidebar_border)
        .child(back(t, cx))
        .child(div().h(px(SPACE[1])))
        .children(Section::ALL.map(|section| nav_row(section, app.ui.settings_section, t, cx)))
}

/// The way out. Settings covers the window, so it owes the window a door.
fn back(t: &Tokens, cx: &mut Context<LgtmApp>) -> Stateful<Div> {
    nav_shell("settings-back", false, t)
        .text_color(t.sidebar_muted)
        .child(icon("arrow-left", ICON, t.sidebar_muted))
        .child("Back to app")
        .on_click(cx.listener(|this, _: &ClickEvent, window, cx| this.close_overlay(window, cx)))
}

fn nav_shell(id: &'static str, selected: bool, t: &Tokens) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(SPACE[1]))
        .h(px(NAV_ROW_H))
        .px(px(SPACE[1]))
        .rounded(px(ROW_RADIUS))
        .cursor_pointer()
        .text_size(px(TEXT_ROW))
        .text_color(t.sidebar_fg)
        .when(selected, |this| this.bg(t.wash))
        .hover(|this| this.bg(t.wash))
}

fn nav_row(
    section: Section,
    current: Section,
    t: &Tokens,
    cx: &mut Context<LgtmApp>,
) -> Stateful<Div> {
    let selected = section == current;
    nav_shell(section.id(), selected, t)
        .child(icon(
            section.icon(),
            ICON,
            match selected {
                true => t.sidebar_fg,
                false => t.sidebar_muted,
            },
        ))
        .child(section.label())
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            this.ui.settings_section = section;
            // The open dropdown belongs to the pane being left.
            this.ui.settings_menu = None;
            cx.notify();
        }))
}

fn pane(app: &LgtmApp, t: &Tokens, cx: &mut Context<LgtmApp>) -> Stateful<Div> {
    let section = app.ui.settings_section;
    div()
        .id("settings-body")
        .flex_1()
        .min_w_0()
        .overflow_y_scroll()
        .track_scroll(&app.ui.settings_scroll)
        .flex()
        .flex_col()
        .items_center()
        .px(px(SPACE[4]))
        .pt(px(BAR_H))
        .pb(px(SPACE[5]))
        .child(
            div()
                .w_full()
                .max_w(px(COLUMN))
                .flex()
                .flex_col()
                .gap(px(SPACE[4]))
                // About names itself with the mark, so it takes no heading.
                .children((section != Section::About).then(|| {
                    div()
                        .text_size(px(TITLE))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(section.label())
                }))
                .children(match section {
                    Section::Appearance => appearance(app, t, cx),
                    Section::Notifications => notifications(t, cx),
                    Section::Models => models(app, t, cx),
                    Section::Orchestrator => orchestrator(app, t, cx),
                    Section::About => about(t),
                }),
        )
}

/// A named card of rows: what turns one pane into a few ideas.
fn group(label: &'static str, rows: Vec<Div>, t: &Tokens) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap(px(SPACE[1]))
        .child(section_label(label, t))
        .child(
            div()
                .flex()
                .flex_col()
                .rounded(px(RADIUS))
                .bg(t.card)
                .border_1()
                .border_color(t.border)
                .children(rows.into_iter().enumerate().map(|(at, row)| {
                    // A hairline between rows, never above the first one.
                    row.when(at > 0, |this| this.border_t_1().border_color(t.border))
                })),
        )
        .into_any_element()
}

/// One row: what it is and what it does on the left, the control on the right.
fn setting(title: &'static str, note: &'static str, t: &Tokens) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(SPACE[3]))
        .px(px(SPACE[2]))
        .py(px(SPACE[1]))
        .min_h(px(48.))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(div().text_size(px(TEXT_ROW)).child(title))
                .when(!note.is_empty(), |this| {
                    this.child(
                        div()
                            .text_size(px(TEXT_SECONDARY))
                            .text_color(t.muted_fg)
                            .child(note),
                    )
                }),
        )
}

/// A value the app reports rather than a control: right-aligned, muted.
fn value(text: impl Into<SharedString>, t: &Tokens) -> Div {
    div()
        .flex_none()
        .max_w(px(280.))
        .truncate()
        .text_size(px(TEXT_SECONDARY))
        .text_color(t.muted_fg)
        .child(text.into())
}

fn appearance(app: &LgtmApp, t: &Tokens, cx: &mut Context<LgtmApp>) -> Vec<AnyElement> {
    let current = crate::theme::pref(cx);
    let style = app.review.style;
    vec![group(
        "Interface",
        vec![
            setting("Theme", "System follows your macOS appearance.", t).child(segmented(
                Pref::ALL
                    .map(|pref| {
                        segment(
                            format!("theme-{}", pref.label()),
                            pref.label(),
                            pref == current,
                            t,
                        )
                        .on_click(cx.listener(
                            move |_, _: &ClickEvent, window, cx| {
                                crate::theme::set_pref(pref, window, cx);
                                cx.notify();
                            },
                        ))
                    })
                    .into_iter(),
            )),
            setting("Diff", "How a changed file is laid out in Review.", t).child(segmented(
                [(DiffStyle::Unified, "Unified"), (DiffStyle::Split, "Split")]
                    .map(|(value, label)| {
                        segment(format!("diff-{label}"), label, style == value, t).on_click(
                            cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.review.set_style(value);
                                cx.notify();
                            }),
                        )
                    })
                    .into_iter(),
            )),
        ],
        t,
    )]
}

fn notifications(t: &Tokens, cx: &mut Context<LgtmApp>) -> Vec<AnyElement> {
    vec![group(
        "Alerts",
        vec![setting(
            "Tell me when a task needs a person",
            "Shown by the system, so it reaches you with LGTM in the background.",
            t,
        )
        .child(
            Switch::new("notify")
                .checked(crate::theme::notify(cx))
                .small()
                .on_click(cx.listener(|_, checked: &bool, _, cx| {
                    crate::theme::set_notify(*checked, cx);
                    cx.notify();
                })),
        )],
        t,
    )]
}

/// The mark over the name over the version: an About page is a title page.
fn about(t: &Tokens) -> Vec<AnyElement> {
    vec![div()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(SPACE[1]))
        .pt(px(SPACE[4]))
        .child(icon("lgtm", MARK, t.fg))
        .child(
            div()
                .pt(px(SPACE[1]))
                .text_size(px(TITLE))
                .font_weight(FontWeight::SEMIBOLD)
                .child("LGTM"),
        )
        .child(
            div()
                .tabular_nums()
                .text_size(px(TEXT_SECONDARY))
                .text_color(t.muted_fg)
                .child(format!("Version {}", env!("CARGO_PKG_VERSION"))),
        )
        .into_any_element()]
}

/// What a task the composer starts is run with, and what the orchestrator
/// this app hosts runs after one ends.
fn models(app: &LgtmApp, t: &Tokens, cx: &mut Context<LgtmApp>) -> Vec<AnyElement> {
    let held = crate::theme::models(cx);
    vec![
        group(
            "Defaults",
            vec![
                setting("Executor", "The harness a new task runs on.", t).child(dropdown(
                    "executor",
                    Pick::of(held.executor),
                    &Pick::HARNESSES,
                    app,
                    t,
                    cx,
                    |models, pick| models.executor = pick.executor().unwrap_or_default(),
                )),
                setting("Model", "Empty leaves the harness on its own default.", t).child(
                    div().w(px(CONTROL_W)).child(
                        Input::new(&app.inputs.model)
                            .bordered(false)
                            .bg(t.input_fill)
                            .rounded(px(RADIUS))
                            .small(),
                    ),
                ),
                setting("Review", "Which harness reviews a finished task.", t).child(dropdown(
                    "review",
                    held.review,
                    &Pick::REVIEW,
                    app,
                    t,
                    cx,
                    |models, pick| models.review = pick,
                )),
            ],
            t,
        ),
        group(
            "Orchestration",
            vec![setting(
                "Orchestrate",
                "Runs a follow-up agent when a task ends. Only while this app hosts the orchestrator, from its next launch.",
                t,
            )
            .child(dropdown(
                "orchestrate",
                held.orchestrate,
                &Pick::ORCHESTRATE,
                app,
                t,
                cx,
                |models, pick| models.orchestrate = pick,
            ))],
            t,
        ),
    ]
}

fn orchestrator(app: &LgtmApp, t: &Tokens, cx: &mut Context<LgtmApp>) -> Vec<AnyElement> {
    let token = match app.link.token_source {
        "LGTM_TOKEN" => "environment (LGTM_TOKEN)".to_string(),
        other => other.to_string(),
    };
    // When this app hosts the orchestrator its own URL is loopback, so the
    // startup-computed line (advertised address) is the one to hand out.
    let join = app
        .link
        .join
        .clone()
        .unwrap_or_else(|| join_line(&app.link.orchestrator, &app.link.token));
    vec![
        group(
            "Connection",
            vec![
                setting("Server", "Where this app sends its work.", t)
                    .child(value(app.link.orchestrator.clone(), t)),
                setting("Status", "", t).child(status(app.link.reachable, app.link.hosted, t)),
                setting("Token", "Where the shared secret came from.", t).child(value(token, t)),
            ],
            t,
        ),
        group(
            "Hosting",
            vec![
                setting(
                    "Run the orchestrator in this app",
                    "Takes effect the next time you open the app.",
                    t,
                )
                .child(
                    Switch::new("embedded-orchestrator")
                        .checked(app.link.embedded)
                        .small()
                        .on_click(cx.listener(|this, checked: &bool, _, cx| {
                            this.link.embedded = *checked;
                            let dir = lgtm_orchestrator::token::data_dir(None);
                            if let Err(e) = crate::save_embedded(&dir, *checked) {
                                this.set_error(format!("cannot save the setting: {e}"), cx);
                            }
                            cx.notify();
                        })),
                ),
                setting(
                    "Add a machine",
                    "Run this on the machine you are adding. It then shows in the sidebar.",
                    t,
                )
                .child(join_control(join, t, cx)),
            ],
            t,
        ),
    ]
}

/// Reachable or not, and which orchestrator it is: one line rather than the
/// two rows that said the same thing twice.
fn status(reachable: bool, hosted: bool, t: &Tokens) -> Div {
    let (tone, word) = match reachable {
        true => (t.success, "Connected"),
        false => (t.danger, "Unreachable"),
    };
    let where_ = match hosted {
        true => "hosted by this app",
        false => "external",
    };
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(SPACE[1]))
        .text_size(px(TEXT_SECONDARY))
        .child(div().w(px(6.)).h(px(6.)).rounded_full().bg(tone))
        .child(word)
        .child(div().text_color(t.muted_fg).child(where_))
}

/// The join line in a mono box with a Copy button: what a person pastes on
/// the machine they are adding.
fn join_control(join: String, t: &Tokens, cx: &mut Context<LgtmApp>) -> Div {
    let copy = join.clone();
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(SPACE[1]))
        .child(
            div()
                .w(px(220.))
                .truncate()
                .px(px(SPACE[1]))
                .py(px(SPACE[0]))
                .rounded(px(ROW_RADIUS))
                .bg(t.muted)
                .font_family(MONO_FONT)
                .text_size(px(TEXT_MONO))
                .child(join),
        )
        .child(
            Button::new("copy-join")
                .label("Copy")
                .small()
                .outline()
                .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()));
                })),
        )
}

/// A few exclusive choices, side by side. No track behind them: the pill on
/// the current one is the whole signal.
fn segmented(children: impl IntoIterator<Item = Stateful<Div>>) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(2.))
        .children(children)
}

fn segment(id: String, label: &'static str, selected: bool, t: &Tokens) -> Stateful<Div> {
    div()
        .id(SharedString::from(id))
        .flex()
        .items_center()
        .justify_center()
        .h(px(CONTROL_H))
        .px(px(SPACE[1]))
        .rounded(px(ROW_RADIUS))
        .cursor_pointer()
        .text_size(px(TEXT_SECONDARY))
        // Colour and fill alone mark the choice. A weight change here would
        // reflow the row under the cursor.
        .text_color(match selected {
            true => t.fg,
            false => t.muted_fg,
        })
        .when(selected, |this| this.bg(t.muted))
        .when(!selected, |this| this.hover(|this| this.text_color(t.fg)))
        .child(label)
}

/// A menu on a trigger: the app's own, since only one of these can be open and
/// the choice list is four items at most.
fn dropdown(
    id: &'static str,
    current: Pick,
    options: &'static [Pick],
    app: &LgtmApp,
    t: &Tokens,
    cx: &mut Context<LgtmApp>,
    set: fn(&mut crate::theme::Models, Pick),
) -> Div {
    let open = app.ui.settings_menu == Some(id);
    div()
        .relative()
        .flex_none()
        .child(trigger(id, current, open, t, cx))
        // Deferred, or the rows under this one would paint over the menu.
        .when(open, |this| {
            this.child(deferred(dismiss(id, cx)))
                .child(deferred(menu(id, current, options, t, cx, set)).with_priority(1))
        })
}

fn trigger(
    id: &'static str,
    current: Pick,
    open: bool,
    t: &Tokens,
    cx: &mut Context<LgtmApp>,
) -> Stateful<Div> {
    div()
        .id(SharedString::from(format!("trigger-{id}")))
        .flex()
        .items_center()
        .gap(px(SPACE[0]))
        .h(px(CONTROL_H))
        .w(px(CONTROL_W))
        .px(px(SPACE[1]))
        .rounded(px(RADIUS))
        .bg(t.input_fill)
        .cursor_pointer()
        .text_size(px(TEXT_SECONDARY))
        .hover(|this| this.bg(t.muted))
        .child(div().flex_1().min_w_0().truncate().child(current.label()))
        .child(icon("chevron-down", ICON, t.muted_fg))
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            let was = this.ui.settings_menu == Some(id);
            this.ui.settings_menu = (!was).then_some(id);
            cx.notify();
        }))
        .when(open, |this| this.bg(t.muted))
}

fn menu(
    id: &'static str,
    current: Pick,
    options: &'static [Pick],
    t: &Tokens,
    cx: &mut Context<LgtmApp>,
    set: fn(&mut crate::theme::Models, Pick),
) -> Div {
    div()
        .absolute()
        .top(px(CONTROL_H + SPACE[0]))
        .left(px(0.))
        .w(px(CONTROL_W))
        .flex()
        .flex_col()
        .p(px(SPACE[0]))
        .rounded(px(RADIUS))
        .bg(t.popover)
        .border_1()
        .border_color(t.border)
        .text_size(px(TEXT_SECONDARY))
        .occlude()
        .children(options.iter().map(|pick| {
            let pick = *pick;
            div()
                .id(SharedString::from(format!("{id}-{}", pick.label())))
                .flex()
                .items_center()
                .gap(px(SPACE[1]))
                .h(px(CONTROL_H))
                .px(px(SPACE[1]))
                .rounded(px(ROW_RADIUS))
                .cursor_pointer()
                .hover(|this| this.bg(t.muted))
                .child(div().flex_1().min_w_0().child(pick.label()))
                .when(pick == current, |this| this.child(icon("check", 14., t.fg)))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    let mut models = crate::theme::models(cx);
                    set(&mut models, pick);
                    crate::theme::set_models(models, cx);
                    this.ui.settings_menu = None;
                    cx.notify();
                }))
        }))
}

/// A click anywhere else closes the open dropdown.
fn dismiss(id: &'static str, cx: &mut Context<LgtmApp>) -> Stateful<Div> {
    div()
        .id(SharedString::from(format!("dismiss-{id}")))
        .absolute()
        .top(px(-4000.))
        .left(px(-4000.))
        .w(px(8000.))
        .h(px(8000.))
        .occlude()
        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
            this.ui.settings_menu = None;
            cx.notify();
        }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_join_line_uses_the_websocket_scheme() {
        assert_eq!(
            join_line("http://127.0.0.1:4750", "abc"),
            "lgtm runner ws://127.0.0.1:4750 --token abc"
        );
        assert_eq!(
            join_line("https://host:4750/", "abc"),
            "lgtm runner wss://host:4750 --token abc"
        );
    }

    #[test]
    fn every_section_carries_its_own_id_and_an_icon_the_app_ships() {
        let ids: Vec<&str> = Section::ALL.iter().map(|it| it.id()).collect();
        let mut unique = ids.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(ids.len(), unique.len());
        assert!(Section::ALL
            .iter()
            .all(|it| crate::assets::NAMES.contains(&it.icon())));
    }
}
