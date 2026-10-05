# User guide

kSEE opens a lesson on the left and its kernel recording on the right. The
usual workflow is to choose or edit a small program, select one of its
syscalls, then move through that syscall while source, stack, tasks, objects,
and console output stay aligned.

## 1. Open a lesson or edit its program

The **Lessons** menu groups lessons by chapter and marks the lesson currently
open. The previous and next buttons move through the course. Opening a lesson
starts a fresh kernel so every lesson begins from the same state.

A lesson contains a question, explanatory paragraphs, and a program. Each
program row is a declaration or syscall. A completed syscall row
shows its result; errors include their kernel name. A row may also show program
variables changed by the call. If a call blocks, its result appears when a
later row wakes it.

Programs with more than one task use a task column for each. The dotted part
of a column means that task is still blocked. Edit a row in place or add a row at the
bottom. Press Enter to run the entire changed program on a fresh kernel. An
invalid row explains why and does not run.

Explanatory links can select a syscall, move directly to a function it called,
switch a detail tab, or run a prepared alternative program. Use **reset** to
return from a try to the lesson's own program.

## 2. Select a syscall and a moment

Selecting a syscall row chooses its recording. kSEE maintains three related
units:

- A **trace record** is a raw event such as a basic block, return, store, task,
  comparison, or variable update.
- A **line step** is a reconstructed source-line position. One basic-block
  record can produce several line steps.
- A **chart column** samples one line step in the current window. A wide window
  has more steps than columns, so a column stands for several.

The chart's axis is line steps: a bar is as wide as the steps its call takes,
and stepping (← →) moves the cursor one step across it.

### Timeline

The narrow overview covers the whole snapshot. It shows which task ran, where
syscalls begin, the visible window, and the cursor. Click it to move the window.

The main chart stacks calls over time, with the outermost call at the bottom.
A bar is an instrumented call or inlined code, colored by the part of the kernel
its source is in (the legend), a concept a color family: files and storage blue
(`fs/`, `block/` deeper), memory green, tasks warm (`kernel/` amber, the
scheduler coral: where a task sleeps and another runs), the machine purple
(`drivers/`, the port's `arch/` paler), and the helpers a cool gray (`lib/`,
`include/`); a file in none of them, gray. Hover to
preview that moment and see what the call was; click to move the cursor
there. The task strip below the chart shows which task ran in each region
(the idle task gray). The cursor is a cross: down, its moment; across, its
frame's level.

A lesson's run shows whole; after zooming in, **all** restores the full
recording.

### Zoom and pan

- Turn the wheel vertically to zoom around the pointer.
- Scroll horizontally, or hold Shift while using a vertical wheel, to pan.
- Drag the chart to pan directly.
- Drag the overview window by clicking where the current-width window should
  move.

The wheel-window calculation lives in `ksee-core`, so native tests cover the
interaction math separately from the WebAssembly page.

### Function search

Type any part of a function name in the box above the chart. Matching bars are
highlighted and the page reports how many calls matched. Press Enter for the
next match or Shift+Enter for the previous one; search wraps at the ends.
Press Escape to clear the query.

## 3. Follow source and stack state

### Source

The source pane follows the selected frame. It shows the complete file and
keeps the current line in view. The current function is edged, and lines that
ran in the selected syscall are marked. Hover a line number to see how often it
ran; click for its next run or Shift+click for its last run.

The current line can show arguments, locals, branch comparisons, and a returned
call result. A storage note at the end of a source line links to that write.
Hover a visible name to inspect its value and members at the selected moment.
Ctrl+click on Windows/Linux, or Command-click on macOS, opens its definition;
use **back to the cursor** to return.

A sealed function is marked in the source header. Its call and return are
recorded, but loop turns inside its body are intentionally omitted because it
writes no reconstructible memory and makes no calls.

### Stack

The Stack tab lists calls at the current moment at the bottom of the pane, the
outermost lowest, as the chart stacks them. Select a frame to show its source.
Under the chosen frame, and only there, are its recorded arguments and locals
as they were just before the current line. Pointers and compound values can be
opened. Hover a value to see its most recent recorded writer and, when the
value was copied, its earlier origins.

## 4. Inspect tasks, objects, values, and output

### Tasks

The Tasks tab reconstructs the task tree at the cursor. It identifies the
running task and labels others as runnable, sleeping, exited, or not yet
created. Open a task to inspect its `task_struct` as memory was at that moment.
A thread appears under its thread-group leader and a process under its parent
when those relationships are available.

### Objects

The Objects tab summarizes kernel data changed by the selected syscall. It
groups changes made by the syscall's own task, changes made by other tasks
while it ran, and values changed and restored before the end. Each object is
named from typed kernel data and arranged under an object that points to it.

A member row shows its before and after values and the function that changed
it. Click the row to move to that write. Objects intentionally omit a call's
ordinary stack locals; those belong in Stack.

### Console

The Console tab contains kernel log messages and program output accumulated
for the current lesson run. The console is output-only.

## 5. Move through the recording

The transport below the chart shows the task, function, file, and line at the
cursor.

| Control | Action |
|---|---|
| Left / Right, or the left/right buttons | Move one reconstructed line step backward or forward. |
| Shift+Left / Shift+Right | Step over calls at the current level. |
| Up / Down, or the up/down buttons | Choose the callee or caller frame. |
| Click a source line number | Run to that line's next occurrence. |
| Shift+click a source line number | Run to that line's previous occurrence. |
| Hover the chart | Preview a moment without moving the cursor. |
| Click the chart | Commit the previewed moment. |

Arrow shortcuts do not take over while focus is in the program editor or
function search field.

## URL state and saved layout

Each lesson has a page of its own, `read.html`; if the program differs from
the lesson's, the fragment holds it as JSON, `read.html#p=[...]`. Sharing that
URL opens the same lesson and changed program; it does not contain a trace
snapshot. (A lesson's page also carries its text as plain HTML, for search
engines and link previews; the page replaces it once it runs.)
The program runs again on a fresh deterministic machine.

Dragged pane sizes and the chosen color theme are kept in browser storage.
