//! The page's layout: the lesson's column, the chart's height, and the
//! tabs' column, each dragged by a split (kept in the browser's storage);
//! while one is dragged, a sheet over the page follows the pointer.

use dioxus::prelude::*;

/// The sizes (px): the lesson's column, the chart's height, the tabs' column.
#[derive(Clone, Copy, PartialEq)]
pub struct Sizes {
    pub left: f64,
    pub chart: f64,
    pub right: f64,
}

impl Default for Sizes {
    fn default() -> Self {
        Sizes { left: 420.0, chart: 230.0, right: 420.0 }
    }
}

/// Which split is dragged, from where, with the sizes then.
#[derive(Clone, Copy, PartialEq)]
pub struct Drag {
    split: Split,
    x: f64,
    y: f64,
    from: Sizes,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Split {
    Left,
    Chart,
    Right,
}

fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

impl Sizes {
    /// As kept, else the default.
    pub fn kept() -> Sizes {
        let v: Vec<f64> = storage().and_then(|s| s.get_item("ksee.sizes").ok().flatten())
            .map(|t| t.split(',').filter_map(|x| x.parse().ok()).collect()).unwrap_or_default();
        match v[..] {
            [left, chart, right] => Sizes { left, chart, right },
            _ => Sizes::default(),
        }
    }

    fn keep(&self) {
        if let Some(s) = storage() {
            let _ = s.set_item("ksee.sizes", &format!("{},{},{}", self.left, self.chart, self.right));
        }
    }

    /// The sizes as the page's CSS variables.
    pub fn style(&self) -> String {
        format!("--left: {}px; --chart: {}px; --right: {}px", self.left, self.chart, self.right)
    }
}

/// A split's handle: pressed, the drag begins.
#[component]
pub fn Handle(split: Split) -> Element {
    let sizes = use_context::<Signal<Sizes>>();
    let mut drag = use_context::<Signal<Option<Drag>>>();
    let class = if split == Split::Chart { "hsplit" } else { "vsplit" };
    rsx! {
        div { class, title: "drag to resize",
            onpointerdown: move |e| {
                e.prevent_default();
                let p = e.client_coordinates();
                drag.set(Some(Drag { split, x: p.x, y: p.y, from: sizes() }));
            }
        }
    }
}

/// While a split is dragged: a sheet over the page, the sizes following
/// the pointer; let go, kept.
#[component]
pub fn Sheet() -> Element {
    let mut sizes = use_context::<Signal<Sizes>>();
    let mut drag = use_context::<Signal<Option<Drag>>>();
    let Some(d) = drag() else { return rsx! {} };
    let cursor = if d.split == Split::Chart { "row-resize" } else { "col-resize" };
    rsx! {
        div { class: "sheet", style: "cursor: {cursor}",
            onpointermove: move |e| {
                let p = e.client_coordinates();
                let (dx, dy) = (p.x - d.x, p.y - d.y);
                let mut s = d.from;
                match d.split {
                    Split::Left => s.left = (d.from.left + dx).clamp(240.0, 900.0),
                    Split::Chart => s.chart = (d.from.chart + dy).clamp(80.0, 800.0),
                    Split::Right => s.right = (d.from.right - dx).clamp(240.0, 900.0),
                }
                sizes.set(s);
            },
            onpointerup: move |_| {
                sizes().keep();
                drag.set(None);
            },
        }
    }
}
