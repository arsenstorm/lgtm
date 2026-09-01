//! The Terminal tab: the shell in the task's worktree, as a terminal — one
//! mono surface bleeding to the pane's edges, scrollback above, prompt below.

use crate::app::LgtmApp;
use crate::theme::{icon_button, Tokens, GLYPH, LINE_MONO, MONO_FONT, SPACE, TEXT_MONO};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, AnyElement, ClickEvent, Context, Div, InteractiveElement as _, IntoElement,
    ParentElement as _, Stateful, StatefulInteractiveElement as _, Styled as _,
};
use gpui_component::input::Input;

/// How much output one attached shell keeps. Dropping the front of a byte
/// buffer is cheap, and nothing above reads what scrolled off.
const SCROLLBACK: usize = 200_000;

pub(super) fn terminal(app: &LgtmApp, t: &Tokens, cx: &mut Context<LgtmApp>) -> AnyElement {
    let attached = app.shell.is_some();
    div()
        .relative()
        .size_full()
        .flex()
        .flex_col()
        .bg(t.composer.rear)
        .font_family(MONO_FONT)
        .text_size(px(TEXT_MONO))
        .line_height(px(LINE_MONO))
        .text_color(t.fg)
        .child(scrollback(app, t))
        .when(attached, |this| {
            this.child(prompt(app, t)).child(close(t, cx))
        })
        .into_any_element()
}

/// The output, and the scrollable `commands.rs` pins to the bottom. Long lines
/// wrap: `strip_ansi` drops cursor movement, so this is an append-only log
/// rather than a grid, and a log must not hide its tail behind a scroll offset.
fn scrollback(app: &LgtmApp, t: &Tokens) -> Stateful<Div> {
    let lines: Vec<Div> = match app.shell.as_ref() {
        None => vec![quiet("Not attached.", t)],
        Some(shell) if shell.output.is_empty() => vec![quiet("Waiting for the shell…", t)],
        Some(shell) => shell
            .output
            .lines()
            .map(|line| div().child(line.to_string()))
            .collect(),
    };
    div()
        .id("terminal-output")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .track_scroll(&app.ui.terminal_scroll)
        .flex()
        .flex_col()
        .pl(px(SPACE[1]))
        // The close cross floats over this corner: keep the text out from under it.
        .pr(px(GLYPH + SPACE[1]))
        .py(px(SPACE[0]))
        .children(lines)
}

fn quiet(text: &str, t: &Tokens) -> Div {
    div().text_color(t.muted_fg).child(text.to_string())
}

/// The bottom line: the same mono text as the output, on the same surface, with
/// one hairline over it. No prompt glyph — the shell prints its own.
fn prompt(app: &LgtmApp, t: &Tokens) -> Div {
    div()
        .flex_shrink_0()
        .border_t_1()
        .border_color(t.border)
        .px(px(SPACE[1]))
        .py(px(SPACE[0]))
        .child(
            Input::new(&app.inputs.shell)
                .appearance(false)
                .p_0()
                .h(px(LINE_MONO))
                .font_family(MONO_FONT)
                .text_size(px(TEXT_MONO))
                .line_height(px(LINE_MONO)),
        )
}

fn close(t: &Tokens, cx: &mut Context<LgtmApp>) -> Stateful<Div> {
    icon_button("close-terminal", "x", true, t)
        .absolute()
        .top(px(SPACE[0]))
        .right(px(SPACE[0]))
        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.close_shell(cx)))
}

impl crate::app::Shell {
    /// Appends a chunk of shell output, capped at [`SCROLLBACK`].
    pub fn push(&mut self, chunk: &str) {
        self.output.push_str(&strip_ansi(chunk));
        if self.output.len() > SCROLLBACK {
            let at = self.output.len() - SCROLLBACK;
            let at = (at..self.output.len())
                .find(|at| self.output.is_char_boundary(*at))
                .unwrap_or(self.output.len());
            self.output.drain(..at);
        }
    }
}

/// Drops the escape sequences a shell paints with, leaving the text they
/// were dressing.
// ponytail: no cursor movement — a sequence that would move, clear, or
// overwrite is dropped, so a program that repaints in place appends instead.
// A real grid emulator is the upgrade if anyone runs a full-screen program.
pub fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\x1b' {
            if c != '\r' && !(c.is_control() && c != '\n' && c != '\t') {
                out.push(c);
            }
            continue;
        }
        match chars.next() {
            // CSI: parameters, then one final byte in `@`..`~`.
            Some('[') => {
                for c in chars.by_ref() {
                    if ('\x40'..='\x7e').contains(&c) {
                        break;
                    }
                }
            }
            // OSC: a string ending at BEL, or at ESC \.
            Some(']') => {
                while let Some(c) = chars.next() {
                    if c == '\x07' {
                        break;
                    }
                    if c == '\x1b' {
                        chars.next();
                        break;
                    }
                }
            }
            // A charset designator carries one more byte with it.
            Some('(') | Some(')') | Some('*') | Some('+') => {
                chars.next();
            }
            // Anything else is a two-byte escape; both are dropped.
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_and_cursor_moves_are_dropped_and_the_text_stays() {
        assert_eq!(strip_ansi("\x1b[31mred\x1b[0m"), "red");
        assert_eq!(strip_ansi("a\x1b[2K\x1b[1;7Hb"), "ab");
        assert_eq!(strip_ansi("plain"), "plain");
    }

    #[test]
    fn window_titles_and_lone_escapes_are_dropped_too() {
        assert_eq!(strip_ansi("\x1b]0;a title\x07done"), "done");
        assert_eq!(strip_ansi("\x1b]0;a title\x1b\\done"), "done");
        assert_eq!(strip_ansi("\x1b(Bx"), "x");
        assert_eq!(strip_ansi("\x1b"), "");
    }

    #[test]
    fn newlines_and_tabs_survive_but_the_other_controls_do_not() {
        assert_eq!(strip_ansi("one\r\ntwo\tthree\x07"), "one\ntwo\tthree");
    }
}
