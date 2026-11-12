#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entry {
    About,
    ChangeServer,
    Hide,
    HideOthers,
    Quit,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
    Minimize,
    Maximize,
    CloseWindow,
    Separator,
}
