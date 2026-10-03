//! Target-neutral rendering for a lesson paragraph.

use dioxus::prelude::*;
use ksee_core::text::{LessonTarget, Piece};

/// A parsed lesson paragraph. Recording links are disabled when the lesson's
/// program is stale; try links remain available.
#[component]
pub(crate) fn ParagraphView(
    pieces: Vec<Piece>,
    stale: bool,
    follow: EventHandler<LessonTarget>,
) -> Element {
    rsx! {
        p { class: "para",
            for piece in pieces {
                match piece {
                    Piece::Text(text) => rsx! { "{text}" },
                    Piece::Code(code) => rsx! { code { "{code}" } },
                    Piece::Link { label, target, .. } => {
                        let parsed = LessonTarget::parse(&target);
                        let class = match &parsed {
                            Ok(LessonTarget::Try { .. }) => "try",
                            Ok(LessonTarget::Recording { function: Some(_), .. }) => "place mono",
                            _ => "",
                        };
                        let recording = matches!(&parsed, Ok(LessonTarget::Recording { .. }));
                        if parsed.is_err() || stale && recording {
                            rsx! { span { class: "dim", "{label}" } }
                        } else {
                            let target = parsed.expect("checked lesson target");
                            rsx! { a { class, onclick: move |_| follow.call(target.clone()), "{label}" } }
                        }
                    }
                }
            }
        }
    }
}
