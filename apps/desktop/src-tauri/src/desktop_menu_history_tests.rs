use super::{desktop_menu_action_id, desktop_menu_items};

#[test]
fn explicit_history_accelerators_dispatch_instead_of_native_document_history() {
    let items = desktop_menu_items();
    for (id, accelerator, action) in [
        ("editor_undo", "CmdOrCtrl+Z", "editorUndo"),
        ("editor_redo", "CmdOrCtrl+Shift+Z", "editorRedo"),
    ] {
        let item = items.iter().find(|item| item.id == id).unwrap();
        assert_eq!(item.menu, "edit");
        assert_eq!(item.accelerator, accelerator);
        assert_eq!(desktop_menu_action_id(id), Some(action));
    }
}
