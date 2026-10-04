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
                    Piece::Link { label, to, .. } => match to {
                        Some(to) if !(stale && matches!(to, LessonTarget::Recording { .. })) => {
                            let class = match &to {
                                LessonTarget::Try { .. } => "try",
                                LessonTarget::Recording { function: Some(_), .. } => "place mono",
                                _ => "",
                            };
                            rsx! { a { class, onclick: move |_| follow.call(to.clone()), "{label}" } }
                        }
                        _ => rsx! { span { class: "dim", "{label}" } },
                    },
                }
            }
        }
    }
}
