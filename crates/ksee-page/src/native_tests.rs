use std::any::Any;
use std::rc::Rc;
use std::sync::Mutex;

use dioxus::dioxus_core::{ElementId, Mutation};
use dioxus::prelude::*;
use dioxus_html::{SerializedHtmlEventConverter, SerializedMouseData};
use ksee_core::text::{self, LessonTarget, Piece};
use ksee_core::view::{Diff, Group, Node, RegionKind, Run, Task};

use crate::lesson_view::ParagraphView;
use crate::objects_view::{ObjectDefinition, ObjectsView};

static PARAGRAPH_TARGETS: Mutex<Vec<LessonTarget>> = Mutex::new(Vec::new());
static OBJECT_RECORDS: Mutex<Vec<usize>> = Mutex::new(Vec::new());

fn paragraph_app() -> Element {
    rsx! {
        ParagraphView {
            pieces: text::pieces("Read [there](2:vfs_read) with `buf`; [try it](try:1), not [bad](wat)."),
            stale: false,
            follow: |target| PARAGRAPH_TARGETS.lock().unwrap().push(target),
        }
    }
}

fn stale_paragraph_app() -> Element {
    rsx! {
        ParagraphView {
            pieces: text::pieces("Read [there](2:vfs_read) or [try it](try:1)."),
            stale: true,
            follow: |_| {},
        }
    }
}

fn line_change(
    member: &str,
    before: &str,
    after: &str,
    first: usize,
    last: usize,
    to: Option<usize>,
) -> Diff {
    Diff {
        member: member.into(),
        before: before.into(),
        after: after.into(),
        to,
        runs: vec![Run {
            before: before.into(),
            after: after.into(),
            writes: 1,
            first,
            last,
            at: None,
        }],
    }
}

fn node(
    label: &str,
    group: Group,
    by: Option<Task>,
    under: Option<usize>,
    changes: Vec<Diff>,
) -> Node {
    Node {
        label: label.into(),
        what: label.into(),
        within: Some("worker".into()),
        kind: RegionKind::Object,
        ty: format!("struct {label}"),
        id: 1,
        addr: 0x1000,
        size: 16,
        changes,
        depth: 0,
        group,
        by,
        under,
        via: under.map(|_| "child".into()),
    }
}

fn object_nodes() -> Vec<Node> {
    vec![
        node(
            "file",
            Group::Changed,
            None,
            None,
            vec![line_change("flags", "0", "1", 3, 4, Some(1))],
        ),
        node(
            "inode",
            Group::Changed,
            None,
            Some(0),
            vec![line_change("mode", "1", "2", 7, 8, None)],
        ),
        node(
            "folio",
            Group::Changed,
            Some(Task {
                addr: 9,
                pid: Some(42),
                comm: Some("journal".into()),
            }),
            None,
            vec![line_change("state", "old", "new", 5, 6, None)],
        ),
        node(
            "lock",
            Group::Same,
            None,
            None,
            vec![line_change("owner", "0", "0", 1, 2, None)],
        ),
    ]
}

fn objects_app() -> Element {
    rsx! {
        ObjectsView {
            nodes: object_nodes().into(),
            title: "open by demo(7)",
            cursor: Some(4),
            definitions: vec![Some(ObjectDefinition { file: "fs/open.c".into(), line: 12 }), None, None, None].into(),
            on_record: |record| OBJECT_RECORDS.lock().unwrap().push(record),
            on_definition: |_| {},
            on_object: |_| {},
        }
    }
}

fn mutation_text(edits: &[Mutation]) -> String {
    edits
        .iter()
        .map(|edit| format!("{edit:?}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn click_listeners(edits: &[Mutation]) -> Vec<ElementId> {
    edits
        .iter()
        .filter_map(|edit| match edit {
            Mutation::NewEventListener { name, id } if name == "click" => Some(*id),
            _ => None,
        })
        .collect()
}

fn click(dom: &VirtualDom, id: ElementId) {
    let event = Event::new(
        Rc::new(PlatformEventData::new(Box::<SerializedMouseData>::default())) as Rc<dyn Any>,
        true,
    );
    dom.runtime().handle_event("click", event, id);
}

#[test]
fn paragraph_renders_piece_semantics_and_stale_links() {
    let mut dom = VirtualDom::new(paragraph_app);
    let edits = dom.rebuild_to_vec();
    let rendered = mutation_text(&edits.edits);
    for expected in [
        "Read ",
        "there",
        "buf",
        "try it",
        "bad",
        "place mono",
        "try",
    ] {
        assert!(
            rendered.contains(expected),
            "missing {expected:?} in mutations:\n{rendered}"
        );
    }
    assert_eq!(
        click_listeners(&edits.edits).len(),
        2,
        "only valid targets should be clickable"
    );

    let mut stale = VirtualDom::new(stale_paragraph_app);
    let stale_edits = stale.rebuild_to_vec();
    assert_eq!(
        click_listeners(&stale_edits.edits).len(),
        1,
        "a stale recording link is disabled but a try remains clickable"
    );
}

#[test]
fn paragraph_clicks_deliver_parsed_targets() {
    PARAGRAPH_TARGETS.lock().unwrap().clear();
    set_event_converter(Box::new(SerializedHtmlEventConverter));
    let mut dom = VirtualDom::new(paragraph_app);
    let edits = dom.rebuild_to_vec();
    for id in click_listeners(&edits.edits) {
        click(&dom, id);
    }
    let targets = PARAGRAPH_TARGETS.lock().unwrap().clone();
    assert!(targets.contains(&LessonTarget::Try { index: 1 }));
    assert!(targets.iter().any(|target| matches!(target, LessonTarget::Recording { call: 2, function: Some(function), .. } if function == "vfs_read")));
}

#[test]
fn objects_group_and_mark_rows_semantically() {
    let mut dom = VirtualDom::new(objects_app);
    let edits = dom.rebuild_to_vec();
    let rendered = mutation_text(&edits.edits);
    for expected in [
        "open by demo(7) changed 2 objects: file, inode",
        "other tasks changed 1 object more",
        "By journal(42), while it ran: 1 object: folio",
        "Left as it was, changed and changed back: lock",
        "obj under",
        "later",
        "on",
        "same",
        "fs/open.c",
        "inode",
    ] {
        assert!(
            rendered.contains(expected),
            "missing {expected:?} in mutations:\n{rendered}"
        );
    }
}

#[test]
fn object_row_click_delivers_record() {
    OBJECT_RECORDS.lock().unwrap().clear();
    set_event_converter(Box::new(SerializedHtmlEventConverter));
    let mut dom = VirtualDom::new(objects_app);
    let edits = dom.rebuild_to_vec();
    for id in click_listeners(&edits.edits) {
        click(&dom, id);
    }
    let records = OBJECT_RECORDS.lock().unwrap().clone();
    assert!(records.contains(&4));
    assert!(records.contains(&8));
    assert!(records.contains(&6));
    assert!(records.contains(&2));
}

#[test]
fn paragraph_accepts_preparsed_pieces() {
    fn app() -> Element {
        rsx! {
            ParagraphView {
                pieces: vec![Piece::Code("direct".into()), Piece::Text(" piece".into())],
                stale: false,
                follow: |_| {},
            }
        }
    }
    let mut dom = VirtualDom::new(app);
    let rendered = mutation_text(&dom.rebuild_to_vec().edits);
    assert!(rendered.contains("direct"));
    assert!(rendered.contains(" piece"));
}
