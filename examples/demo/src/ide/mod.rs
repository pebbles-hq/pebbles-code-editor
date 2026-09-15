//! A full IDE built on Pebbles + `pebbles-code-editor`, as a reference for what the framework
//! can do: an activity bar, a file explorer / project search / settings sidebar, editor tabs
//! with dirty state, a fully-wired code editor, a Problems dock, a menu bar, and a status bar.
//!
//! Architecture: [`Workspace`] (files + open tabs + active) and [`Settings`] are `Copy`
//! reactive state created once here and handed to each panel; every panel is a pure(-ish)
//! builder over that state. One concern per module.

mod editor_pane;
mod explorer;
mod problems;
mod providers;
mod search_panel;
mod settings;
mod statusbar;
mod tabs;
mod widgets;
mod workspace;

use pebbles::prelude::*;

use crate::samples;
use settings::Settings;
use std::rc::Rc;
use workspace::{FileEntry, Files, Workspace};

/// Which sidebar panel is showing.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Panel {
    Explorer,
    Search,
    Settings,
}

// Fixed chrome heights (used to size the scrolling editor viewport).
const MENUBAR_H: f64 = 34.0;
const TABBAR_H: f64 = 38.0;
const STATUS_H: f64 = 24.0;
const PROBLEMS_H: f64 = 180.0;

/// The IDE root component.
pub fn ide() -> AnyWidget {
    // Build the project file set once. Each file gets a live buffer signal and its own
    // diagnostics/inlays signals kept current by a single per-file effect — created ONCE here
    // (never inside a per-render helper), so nothing churns the hook order as you navigate.
    let files: Files = Rc::new(
        samples::project()
            .into_iter()
            .map(|s| {
                let content = create_signal(s.content.clone());
                let diagnostics = create_signal(providers::compute_diagnostics(&s.content));
                let inlays = create_signal(providers::compute_inlays(&s.content));
                create_effect(move || {
                    let src = content.get();
                    diagnostics.set(providers::compute_diagnostics(&src));
                    inlays.set(providers::compute_inlays(&src));
                });
                FileEntry {
                    path: s.path.to_string(),
                    name: s.path.rsplit('/').next().unwrap_or(s.path).to_string(),
                    lang: s.lang,
                    content,
                    saved: create_signal(s.content),
                    diagnostics,
                    inlays,
                    scroll: create_signal(0.0),
                    folds: create_signal(std::collections::BTreeSet::new()),
                }
            })
            .collect(),
    );
    let ws = Workspace::new(files);
    let settings = Settings::new();
    let panel = create_signal(Panel::Explorer);
    let problems_open = create_signal(true);

    // Open a couple of files on launch (src/main.rs active) so the editor shows real code.
    ws.open_file(0);
    ws.open_file(3);

    component_props(
        render_shell,
        ShellProps {
            ws,
            s: settings,
            panel,
            problems_open,
        },
    )
    .into_widget()
}

/// The reactive shell's props (state created once in [`ide`]).
struct ShellProps {
    ws: Workspace,
    s: Settings,
    panel: Signal<Panel>,
    problems_open: Signal<bool>,
}

fn render_shell(p: &ShellProps) -> AnyWidget {
    shell(p.ws.clone(), p.s, p.panel, p.problems_open)
}

fn shell(
    ws: Workspace,
    s: Settings,
    panel: Signal<Panel>,
    problems_open: Signal<bool>,
) -> AnyWidget {
    let c = theme().colors;

    // Available editor height = window − chrome − (problems dock, if open).
    let win_h = media_query().size.height;
    let editor_h = win_h
        - MENUBAR_H
        - TABBAR_H
        - STATUS_H
        - if problems_open.get() { PROBLEMS_H } else { 0.0 };

    // Sidebar content for the selected activity.
    let sidebar_body = match panel.get() {
        Panel::Explorer => explorer::panel(ws.clone()),
        Panel::Search => search_panel::panel(ws.clone()),
        Panel::Settings => settings::panel(s),
    };
    let sidebar = container()
        .width(260.0)
        .decoration(BoxDecoration::new().color(c.card).border(Border::only(
            BorderSide::NONE,
            BorderSide::new(c.border, 1.0),
            BorderSide::NONE,
            BorderSide::NONE,
        )))
        .child(sidebar_body);

    // Editor column: tabs, the editor, and (optionally) the Problems dock.
    let mut editor_col: Vec<AnyWidget> = vec![
        tabs::bar(ws.clone()),
        expanded(editor_pane::pane(ws.clone(), s, editor_h)).into_widget(),
    ];
    if problems_open.get() {
        editor_col.push(
            container()
                .height(PROBLEMS_H)
                .decoration(
                    BoxDecoration::new()
                        .color(c.background)
                        .border(Border::only(
                            BorderSide::new(c.border, 1.0),
                            BorderSide::NONE,
                            BorderSide::NONE,
                            BorderSide::NONE,
                        )),
                )
                .child(problems::dock(ws.clone()))
                .into_widget(),
        );
    }
    let editor_area = expanded(
        column(editor_col)
            .cross_axis_alignment(CrossAxisAlignment::Stretch)
            .main_axis_size(MainAxisSize::Max),
    );

    let middle = expanded(
        row(children![
            activity_bar(panel, problems_open),
            sidebar,
            editor_area,
        ])
        .cross_axis_alignment(CrossAxisAlignment::Stretch),
    );

    container()
        .color(c.background)
        .child(
            column(children![
                menu_bar(ws.clone(), s, problems_open),
                middle,
                statusbar::bar(ws.clone(), s),
            ])
            .cross_axis_alignment(CrossAxisAlignment::Stretch)
            .main_axis_size(MainAxisSize::Max),
        )
        .into_widget()
}

/// The left icon rail: Explorer / Search / Settings, plus a Problems-panel toggle.
fn activity_bar(panel: Signal<Panel>, problems_open: Signal<bool>) -> AnyWidget {
    let c = theme().colors;
    let item = move |ic: pebbles::render::IconData, p: Panel| {
        let active = panel.get() == p;
        let col = if active {
            c.foreground
        } else {
            c.muted_foreground
        };
        GestureDetector::new(
            container()
                .width(48.0)
                .height(46.0)
                .decoration(BoxDecoration::new().border(Border::only(
                    BorderSide::NONE,
                    BorderSide::NONE,
                    BorderSide::NONE,
                    BorderSide::new(
                        if active {
                            c.accent
                        } else {
                            Color::from_rgba8(0, 0, 0, 0)
                        },
                        2.0,
                    ),
                )))
                .alignment(Alignment::CENTER)
                .child(icon(ic).size(21.0).color(col)),
        )
        .cursor(Cursor::Pointer)
        .on_tap(action(move || panel.set(p)))
        .into_widget()
    };

    let problems_btn = {
        let on = problems_open.get();
        GestureDetector::new(
            container()
                .width(48.0)
                .height(46.0)
                .alignment(Alignment::CENTER)
                .child(icon(tabler::LAYOUT_BOTTOMBAR).size(20.0).color(if on {
                    c.foreground
                } else {
                    c.muted_foreground
                })),
        )
        .cursor(Cursor::Pointer)
        .on_tap(action(move || problems_open.set(!problems_open.peek())))
        .into_widget()
    };

    container()
        .width(48.0)
        .decoration(BoxDecoration::new().color(c.card).border(Border::only(
            BorderSide::NONE,
            BorderSide::new(c.border, 1.0),
            BorderSide::NONE,
            BorderSide::NONE,
        )))
        .child(
            column(children![
                item(tabler::FILES, Panel::Explorer),
                item(tabler::SEARCH, Panel::Search),
                item(tabler::SETTINGS, Panel::Settings),
                spacer(),
                problems_btn,
            ])
            .cross_axis_alignment(CrossAxisAlignment::Center)
            .main_axis_size(MainAxisSize::Max),
        )
        .into_widget()
}

/// The top menu bar (File / Edit / View). Edit routes to the focused editor via key dispatch.
fn menu_bar(ws: Workspace, s: Settings, problems_open: Signal<bool>) -> AnyWidget {
    let c = theme().colors;
    use pebbles::core::focus::dispatch_key;

    let ws_save = ws.clone();
    let ws_close = ws.clone();
    let ws_closeall = ws.clone();
    let file_menu = vec![
        menu_item("Save")
            .shortcut("Ctrl+S")
            .on_select(move || {
                if let Some(i) = ws_save.active.peek() {
                    ws_save.save(i);
                }
            })
            .into(),
        menu_item("Close Tab")
            .on_select(move || {
                if let Some(i) = ws_close.active.peek() {
                    ws_close.close_file(i);
                }
            })
            .into(),
        menu_separator(),
        menu_item("Close All")
            .on_select(move || {
                ws_closeall.open.set(Vec::new());
                ws_closeall.active.set(None);
            })
            .into(),
    ];

    let edit_menu = vec![
        menu_item("Undo")
            .shortcut("Ctrl+Z")
            .on_select(|| {
                dispatch_key(KeyInput::Undo);
            })
            .into(),
        menu_item("Redo")
            .shortcut("Ctrl+Y")
            .on_select(|| {
                dispatch_key(KeyInput::Redo);
            })
            .into(),
        menu_separator(),
        menu_item("Find")
            .shortcut("Ctrl+F")
            .on_select(|| {
                dispatch_key(KeyInput::Find);
            })
            .into(),
        menu_item("Replace")
            .shortcut("Ctrl+H")
            .on_select(|| {
                dispatch_key(KeyInput::Replace);
            })
            .into(),
        menu_item("Command Palette")
            .shortcut("Ctrl+P")
            .on_select(|| {
                dispatch_key(KeyInput::CommandPalette);
            })
            .into(),
        menu_separator(),
        menu_item("Format Document")
            .shortcut("Shift+Alt+F")
            .on_select(|| {
                dispatch_key(KeyInput::Format);
            })
            .into(),
    ];

    let (mm, sk, gd) = (s.minimap, s.sticky, s.guides);
    let (wp, lt) = (s.whitespace, s.light);
    let view_menu = vec![
        menu_check("Minimap", mm.get(), move |v| mm.set(v)),
        menu_check("Sticky scroll", sk.get(), move |v| sk.set(v)),
        menu_check("Indent guides", gd.get(), move |v| gd.set(v)),
        menu_check("Whitespace", wp.get(), move |v| wp.set(v)),
        menu_separator(),
        menu_check("Light theme", lt.get(), move |v| lt.set(v)),
        menu_check("Problems panel", problems_open.get(), move |v| {
            problems_open.set(v)
        }),
    ];

    container()
        .height(MENUBAR_H)
        .decoration(BoxDecoration::new().color(c.card).border(Border::only(
            BorderSide::NONE,
            BorderSide::NONE,
            BorderSide::new(c.border, 1.0),
            BorderSide::NONE,
        )))
        .padding(EdgeInsets::symmetric(6.0, 0.0))
        .alignment(Alignment::CENTER_LEFT)
        .child(
            row(children![
                container().padding(EdgeInsets::symmetric(8.0, 0.0)).child(
                    text("Pebbles IDE")
                        .size(12.5)
                        .weight(700.0)
                        .color(c.foreground)
                ),
                menubar()
                    .menu("File", file_menu)
                    .menu("Edit", edit_menu)
                    .menu("View", view_menu),
            ])
            .cross_axis_alignment(CrossAxisAlignment::Center),
        )
        .into_widget()
}
