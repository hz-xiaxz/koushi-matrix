use std::collections::{HashMap, HashSet};

use super::space_selection::*;

#[derive(Default)]
struct FakeSpaces {
    children: HashMap<String, Result<Vec<ChildEntry>, SelectionError>>,
    dms: HashSet<String>,
    visited: Vec<String>,
}

impl FakeSpaces {
    fn space(mut self, id: &str, children: Vec<ChildEntry>) -> Self {
        self.children.insert(id.to_owned(), Ok(children));
        self
    }

    fn failing(mut self, id: &str) -> Self {
        self.children.insert(id.to_owned(), Err(SelectionError));
        self
    }

    fn dm(mut self, id: &str) -> Self {
        self.dms.insert(id.to_owned());
        self
    }
}

impl SpaceChildSource for FakeSpaces {
    async fn children(&mut self, space_id: &str) -> Result<Vec<ChildEntry>, SelectionError> {
        self.visited.push(space_id.to_owned());
        self.children.get(space_id).cloned().unwrap_or(Ok(Vec::new()))
    }

    async fn is_dm(&mut self, room_id: &str) -> bool {
        self.dms.contains(room_id)
    }
}

fn room(id: &str, joined: bool) -> ChildEntry {
    ChildEntry { room_id: id.to_owned(), display_name: format!("name {id}"), joined, is_space: false }
}

fn space(id: &str, joined: bool) -> ChildEntry {
    ChildEntry { room_id: id.to_owned(), display_name: format!("name {id}"), joined, is_space: true }
}

fn select(source: &mut FakeSpaces, root: &str) -> Result<Vec<(String, bool)>, SelectionError> {
    let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
    runtime
        .block_on(select_space_rooms(source, root))
        .map(|rooms| rooms.into_iter().map(|room| (room.room_id, room.target)).collect())
}

#[test]
fn selection_recurses_joined_subspaces_and_skips_unjoined() {
    let mut source = FakeSpaces::default()
        .space("!root", vec![room("!a", true), space("!sub", true), space("!closed", false), room("!b", false)])
        .space("!sub", vec![room("!c", true)])
        .space("!closed", vec![room("!hidden", true)]);
    let rooms = select(&mut source, "!root").unwrap();
    assert_eq!(
        rooms,
        vec![
            ("!a".to_owned(), true),
            ("!closed".to_owned(), false),
            ("!b".to_owned(), false),
            ("!c".to_owned(), true),
        ]
    );
    assert!(!source.visited.contains(&"!closed".to_owned()), "unjoined subspaces are not traversed");
}

#[test]
fn selection_excludes_dms() {
    let mut source = FakeSpaces::default()
        .space("!root", vec![room("!a", true), room("!dm", true)])
        .dm("!dm");
    assert_eq!(select(&mut source, "!root").unwrap(), vec![("!a".to_owned(), true)]);
}

#[test]
fn selection_visits_each_room_once_through_cycles() {
    let mut source = FakeSpaces::default()
        .space("!a", vec![room("!r", true), space("!b", true)])
        .space("!b", vec![room("!r", true), space("!a", true), room("!s", true)]);
    assert_eq!(
        select(&mut source, "!a").unwrap(),
        vec![("!r".to_owned(), true), ("!s".to_owned(), true)]
    );
    assert_eq!(source.visited, vec!["!a".to_owned(), "!b".to_owned()]);
}

#[test]
fn selection_fails_when_the_root_cannot_be_read() {
    let mut source = FakeSpaces::default().failing("!root");
    assert_eq!(select(&mut source, "!root"), Err(SelectionError));
}

#[test]
fn a_failing_subspace_is_skipped_and_the_rest_continues() {
    let mut source = FakeSpaces::default()
        .space("!root", vec![space("!sub", true), room("!a", true)])
        .failing("!sub");
    assert_eq!(select(&mut source, "!root").unwrap(), vec![("!a".to_owned(), true)]);
}

#[test]
fn selected_rooms_carry_display_names() {
    let mut source = FakeSpaces::default().space("!root", vec![room("!a", true)]);
    let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
    let rooms = runtime.block_on(select_space_rooms(&mut source, "!root")).unwrap();
    assert_eq!(rooms[0].display_name, "name !a");
}
