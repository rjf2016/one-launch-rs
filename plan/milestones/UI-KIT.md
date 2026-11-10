# UI kit and shell track

Runs in parallel from Milestone 1 to Milestone 4. It needs only frozen DTOs and a mock backend. U1 to U4, U8 and U9 are due before the Steam slice's UI ticket; the rest are due by the Milestone 4 gate.

`E\` is the Electron repo at the pin. Design: `../design/architecture.md` (UI, Automation).

| Ticket | Scope | Ports |
| --- | --- | --- |
| U1 | Tokens, type scale, focus ring, icons | `E\src\renderer\src\index.css`, `oracle/assets` |
| U2 | Button, Toggle, Tabs, StatusPill, Panel, Field, PageHeader, EmptyState, Collapsible | `E\src\renderer\src\components\*` |
| U3 | Popup engine, Dropdown, MenuButton, CardMenu | `components\{Dropdown,MenuButton,CardMenu}.tsx` |
| U4 | Modal layer, focus trap, ConfirmDialog | `components\ConfirmDialog.tsx` |
| U5 | TextField, Textarea, TagInput (on the Kit input) | `components\{Field,TagInput}.tsx` |
| U6 | Scroll, HScroller, tooltip | `components\HScroller.tsx`, `lib\scrollbarHover.ts` |
| U7 | Window shell, title bar, placement memory, Sidebar, shortcuts | `E\src\main\window-guard.ts`, `components\{TitleBar,Sidebar}.tsx`, `lib\shortcuts.ts` |
| U8 | Stores, `Query`, Api client, MockBackend | `store\ui.ts`, `lib\*` |
| U9 | Automation server and the UI-parity harness | new |
| U10 | Tray, close-to-tray, hidden start, single instance, quit state machine, native dialogs, crash dialog | `E\src\main\index.ts`, `tray.ts` |
| U11 | Component gallery and baselines | new |

Every component takes a stable id, role and label from the start; the automation tree and the parity sidecar depend on it. Each new pattern lands in the GPUI cookbook, and screen tickets copy from the cookbook rather than inventing.

## Exit

- The gallery's sidecars are within tolerance of the Electron baselines.
- The `window-guard` vectors pass.
