# UI & Feature-Completeness Standards — Gravel (and Pebbles widgets)

**Status: MANDATORY**, alongside `coding-standards.md` (performance and memory). Every agent and human must follow both. A feature that breaks a **MUST** rule here is **not done**, even if its logic works and its tests pass.

Why this exists: features have been shipped that "work" but feel broken:
- no hover or press feedback;
- misaligned rows;
- text overflowing into other columns;
- dead buttons;
- panels that duplicate each other;
- missing empty and error states;
- menus with items that do nothing.

Each rule below is something the owner has had to report by hand. The bar is **VS Code / JetBrains / SourceGit polish**, in Gravel's own look (Gravel Dark, islands layout).

Words: **MUST / MUST NOT** = hard rule. **SHOULD** = default; deviate only with a written reason in the commit message.

---

## Part A — Feature completeness

A1. **Definition of done.** A feature is done only when all of these hold:
- every item in its spec or checklist works **end to end in the real app**: a real click, the real external tool or a faithful fake, and the result visible on screen;
- every state is handled: loading, empty, error, success, disabled;
- every control has feedback (Part B);
- it matches the house style of existing Gravel views (Part C);
- it's keyboard accessible (Part D);
- it's tested with **real pointer and key events through hit-testing** (Part F);
- it meets `coding-standards.md`;
- the checklist is ticked, with the commit SHA and a screenshot.

A2. **No fake or stub functionality.** MUST NOT ship:
- a button, menu item, command, setting or toggle that does nothing, does the wrong thing, or only logs;
- placeholder text like "TODO" or "Coming soon".

If something can't be done yet, it's **not shown**, and it's listed as an open item in the checklist.

A3. **No partial phases.** A phase ships complete. If it can't be finished, say exactly what's missing in the report and leave those items **unticked**. Never tick an item that isn't working in the real app.

A4. **Every action gives an outcome.** Success is visible: the list updates, the row changes state, or a short toast for things that aren't otherwise visible. Failure gives a toast or inline message with the tool's actual error text, and the details go into the relevant output channel. Silent failure is a P0 bug.

A5. **Long operations show progress.** Anything over ~300 ms shows a spinner or progress bar on the row, button or section, plus a status-bar busy indicator for long jobs. The control that started it is disabled meanwhile, to prevent double runs. Cancellable operations offer Cancel.

A6. **Destructive actions confirm.** Delete, remove, prune, discard, force and reset get a confirm dialog that:
- names exactly what will be affected, with counts and sizes;
- puts the destructive button on the right, in danger colour;
- has a default that is safe;
- skips the confirm only where the user opted out ("Don't ask again" stored in settings).

A7. **Discoverability.** Every action is reachable from:
- the place a user looks for it: a row's context menu, or a section or toolbar button;
- the command palette, with a clear `Category: Action` title.

Common actions also have a keybinding matching VS Code where one exists. Tooltips on every icon-only button show the name, plus the shortcut if any.

A8. **Consistency with VS Code.** For any feature VS Code or a well-known extension has (Docker, Git, debugger, tasks), match its terms, menu order, default shortcuts and behaviour, unless the owner decided otherwise. Read the reference before building.

A9. **No duplication.** The same list or data MUST NOT appear in two places at once. One home per thing; other places link to it.

> Owner complaint: Stashes and Local Changes shown both in the side bar and in the Git tab.

A10. **Respect the scope.** Build what the spec asks for, fully. Don't add half-done extras. Anything outside scope goes in the checklist's "Later" list, not into the UI.

---

## Part B — Interaction feedback (the "effects")

Every interactive element MUST visibly react. Use the shared theme tokens and helpers (see `src/ui/ide_theme.rs`, `list_row` in `src/ui/scm.rs`, Pebbles' button and list components). Never hard-coded colours.

B1. **Hover:** rows, buttons, icon buttons, tabs, menu items, links and section headers show a hover background or underline. The pointer cursor changes to a hand on clickable items and to text on inputs. Hover MUST be cheap (`coding-standards.md` §2.3): one row re-renders.

B2. **Pressed:** buttons show a pressed state while the pointer is down.

B3. **Selected:** clicking a row selects it with the theme's selection colour. With multi-select lists, Ctrl/Shift+click selects several. The selection survives refreshes; it's keyed by id, not position.

B4. **Focus:** keyboard focus is always visible, with a focus ring or outline on the focused control or row. Focus MUST NOT be lost or stolen unexpectedly, for example by a refresh, an incoming toast or a background update.

B5. **Disabled:** unavailable actions are shown disabled (dimmed, not-allowed cursor, tooltip saying why), or hidden if they never apply in that context. Never enabled-but-failing.

B6. **Row actions:** inline action icons on rows appear on hover or selection, right-aligned. They MUST NOT cover the row's main text; the text ellipsizes to make room.

B7. **Transitions:** expand/collapse chevrons rotate, panels and popups appear with the standard Pebbles animation, and nothing pops or jumps. Animations respect timing (`create_blink` and similar), never per-frame loops (coding standard §4.4).

B8. **Double-click** does the row's primary action, such as open, view or check out, as in VS Code.

---

## Part C — Visual layout & consistency

C1. **Reuse, don't reinvent.** New views MUST be built from existing Gravel and Pebbles building blocks: list rows, section headers with counts and collapse, toolbar or title-bar buttons, the filter box, empty states, confirm dialogs, toasts, quick picks, input boxes, read-only editor tabs, terminal tabs. If a block is missing or wrong, fix or extend it in Pebbles (upstream), then use it. Don't copy-paste a one-off.

C2. **Measurements** follow `default-design-sizing-requirements.md` and match neighbouring views:
- list rows are the same height as the Explorer and Source Control rows;
- side-bar padding, indent per tree level, icon size (16 px in lists), gap between icon and label, and section header height all match;
- typography uses the same font, sizes and weights as the rest of the side bar. No new sizes.

C3. **Alignment:**
- icons and text share a baseline and are vertically centred in the row;
- numbers, status badges and counts are right-aligned;
- columns line up with their headers;
- trailing actions are pinned to the right edge.

C4. **Overflow:**
- text never wraps unexpectedly or spills into other columns or rows; it uses one line plus an ellipsis, with the full text in a tooltip;
- panels work at narrow widths (240 px side bar) and at 1.0×, 1.25× and 1.5× scale;
- nothing gets clipped off-screen.

> Owner complaint: the Git graph message column overflowing into the author column.

C5. **Colours** come from the theme: foreground, muted, accent, selection, hover, border, and the semantic success, warning and danger colours. Status colours carry meaning and are used consistently. For example, running is green, stopped is muted grey, error is red, paused is amber. Status MUST also be readable without colour, through an icon or text.

C6. **Icons:**
- from the shared icon set (Tabler or Codicon equivalents), the same style and size as the rest of the UI;
- each icon means one thing everywhere;
- no emoji;
- no mismatched or tiny/huge icons.

C7. **Backgrounds and surfaces:**
- panels, empty areas and editor-like areas use the correct surface token (editor background versus side-bar background);
- no odd lighter or darker blocks;
- dividers are 1 px, in the theme border colour.

> Owner complaints: the lighter grey empty diff area; no divider in the side-by-side diff.

C8. **Dialogs and popups:**
- consistent padding (8 px or more), title aligned with the content;
- primary button on the right;
- Esc closes and Enter confirms;
- the dialog fits its content.

> Owner complaint: dialog titles touching the edges.

C9. **Every state has a designed look:**
- **Loading:** spinner or skeleton, no blank panel.
- **Empty:** a friendly message plus the obvious next action, e.g. "No containers. Run an image…".
- **Error:** what went wrong, the tool's message, and a Retry or Fix action.
- **Not available:** a clear explanation with a docs link, e.g. "Docker isn't installed".

C10. **Screenshots are part of review.** Every UI commit attaches lavapipe captures at 1.0× and 1.5× of each new state: normal, hover, selected, empty and error. Compare them side by side with the nearest existing Gravel view. If it looks different from its neighbours without a reason, it's wrong.

---

## Part D — Keyboard & accessibility

D1. Every list is keyboard-navigable:
- Up/Down move;
- Left/Right collapse and expand;
- Enter runs the primary action;
- Space selects;
- Home/End/PageUp/PageDown work;
- typing a letter jumps to the first match.

The focused row is scrolled into view.

D2. Every action in a context menu is also reachable with the keyboard, through Shift+F10 or the Menu key on the focused row, and through the palette.

D3. Tab order is logical, and Esc closes popups and cancels inline edits.

D4. Every interactive element has an accessible name (semantics label), which the tests also use to find it.

---

## Part E — Data correctness & behaviour

E1. Lists reflect reality. They update after every action and on external changes:
- through events or watchers, never polling (`coding-standards.md` §4);
- results from stale requests are dropped, so nothing older overwrites something newer.

E2. Sorting and grouping are stable and sensible: running before stopped, by name, grouped by project. Newly added items don't make the list jump under the cursor.

E3. Filtering and search:
- filter boxes filter as you type, are case-insensitive and highlight the matching text;
- clearing the filter restores the full list;
- a filter that matches nothing shows "No results for '…'".

E4. State persists where users expect it: section collapse states, column widths, sort order, the selected page and splitter sizes, per workspace, restored on restart.

E5. Inputs validate before running: required fields, formats such as `name:tag` or `host:container` ports, and duplicate names. Errors show inline, next to the field.

E6. Copy actions copy exactly what they say. Open-in-browser and open-file actions open the right target.

---

## Part F — Tests that prove it works

F1. Every user-facing action has an integration test that:
- drives the **real UI with pointer and key events through hit-testing**, as in `tests/scm_clicks.rs` and `tests/menus_*.rs`, never by calling handlers directly;
- asserts the visible result on screen;
- asserts the external effect, using a temp repo or workspace and a faithful fake CLI on PATH. Never the network.

F2. Every context-menu item and toolbar button is clicked in a test (the menu-audit pattern). A menu item without a test doesn't ship.

F3. Every state in C9 is tested: loading, empty, error and not-available.

F4. Visual checks run on lavapipe: pixel or layout assertions for alignment, overflow, hover and selection colours, and dividers. Keep captures in `documentations/evidence/<feature>/`.

F5. Keyboard navigation has a test for each list (D1).

---

## Part G — Delivery checklist (paste into the commit/PR and tick)

```
UI & completeness checklist (documentations/ui-feature-standards.md)
[ ] Every spec item works end to end in the real app; nothing stubbed, no dead controls (A1–A3)
[ ] Every action shows success/failure; long ops show progress; destructive ops confirm (A4–A6)
[ ] Reachable from context menu/toolbar AND palette; tooltips; VS Code terms/shortcuts (A7–A8)
[ ] No duplicated views; scope respected (A9–A10)
[ ] Hover / pressed / selected / focus / disabled states on every control; row actions don't cover text (B1–B6)
[ ] Built from shared components; sizes, alignment, typography match neighbours (C1–C3)
[ ] No overflow at 240 px / 1.5×; theme colours and icons only; correct surfaces and dividers (C4–C7)
[ ] Dialogs consistent; loading / empty / error / not-available states designed (C8–C9)
[ ] Screenshots attached at 1.0× and 1.5× of each state, compared with neighbouring views (C10)
[ ] Keyboard navigation, menu key, Esc/Enter, accessible names (D)
[ ] Lists live (events), stable order, filter highlighting, persisted UI state, validated inputs (E)
[ ] Real-event tests for every action, menu item and state; pixel checks for layout (F)
[ ] coding-standards.md §9 checklist also ticked
```

A feature that can't tick a box MUST say why in the commit message and leave the corresponding checklist item unticked.
