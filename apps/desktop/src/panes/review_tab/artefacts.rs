use crate::app::LgtmApp;
use crate::theme::{section, Tokens, SPACE, TEXT_SECONDARY};
use gpui::{div, img, px, Context, Div, Image, ParentElement as _, Styled as _};
use lgtm_protocol::{StoredEvent, TaskEvent};
use std::sync::Arc;

/// The files the runs left for whoever reviews the task. The events carry
/// only a name and a size; an image's bytes are fetched once, on the first
/// render that wants them.
pub(super) fn render(app: &LgtmApp, t: &Tokens, cx: &mut Context<LgtmApp>) -> Option<Div> {
    let found = latest(&app.events);
    if found.is_empty() {
        return None;
    }
    let task = app.selected.clone().unwrap_or_default();
    Some(
        section("Artefacts", t).children(found.into_iter().map(|(name, size)| {
            let held = app.artefacts.get(&(task.clone(), name.clone()));
            if held.is_none() && crate::app::artefact_format(&name).is_some() {
                fetch(name.clone(), cx);
            }
            row(&name, size, held.cloned().flatten(), t)
        })),
    )
}

/// The app is borrowed for the whole render, so the request is made once it
/// is free again.
fn fetch(name: String, cx: &mut Context<LgtmApp>) {
    let view = cx.entity();
    cx.defer(move |cx| {
        view.update(cx, |this, cx| {
            this.want_artefact(name);
            cx.notify();
        })
    });
}

/// One entry per name, the last event's size: a run that overwrites its
/// screenshot every time has one artefact, not one per run.
fn latest(events: &[StoredEvent]) -> Vec<(String, usize)> {
    let mut out = Vec::new();
    for stored in events {
        let TaskEvent::Artefact { name, size, .. } = &stored.event else {
            continue;
        };
        match out.iter_mut().find(|(known, _)| known == name) {
            Some((_, known_size)) => *known_size = *size,
            None => out.push((name.clone(), *size)),
        }
    }
    out
}

fn row(name: &str, size: usize, image: Option<Arc<Image>>, t: &Tokens) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(SPACE[0]))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(SPACE[1]))
                .child(div().text_color(t.fg).child(name.to_string()))
                .child(
                    div()
                        .text_size(px(TEXT_SECONDARY))
                        .text_color(t.muted_fg)
                        .child(size_label(size)),
                ),
        )
        .children(image.map(|image| img(image).max_w_full().max_h(px(320.))))
}

fn size_label(size: usize) -> String {
    if size < 1024 {
        return format!("{size} B");
    }
    format!("{:.1} kB", size as f32 / 1024.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn artefact(size: usize) -> StoredEvent {
        StoredEvent {
            at: 0,
            event: TaskEvent::Artefact {
                name: "shot.png".into(),
                size,
                bytes_base64: String::new(),
            },
        }
    }

    #[test]
    fn an_artefact_sent_twice_is_listed_once() {
        let found = latest(&[artefact(3), artefact(2)]);

        assert_eq!(found, vec![("shot.png".into(), 2)]);
        assert!(crate::app::artefact_format("shot.png").is_some());
        assert!(crate::app::artefact_format("notes.txt").is_none());
    }
}
