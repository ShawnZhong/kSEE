# User guide

kSEE opens a lesson on the left and its kernel recording on the right. The
usual workflow is to choose or edit a small program, select one of its
syscalls, then move through that syscall while source, stack, memory, tasks,
disk and console output stay aligned.

## 1. Open a lesson or edit its program

**☰ Lessons** lists every lesson by chapter, over the lesson's side, the
lesson open marked; ◀ and ▶ beside it, and the lessons before and after
named at the foot of the lesson's side, move through the course. Opening a
lesson starts a fresh kernel so every lesson begins from the same state.

A lesson contains explanatory paragraphs and its program, after them (kept
in view while the text is longer than the side). Each
program row is a declaration or syscall. A completed syscall row
shows its result; errors include their kernel name. A row may also show program
variables changed by the call. If a call blocks, its result appears when a
later row wakes it.

Programs with more than one task use a task column for each. The dotted part
of a column means that task is still blocked. Edit a row in place or add a row at the
bottom. A row changed runs the entire program again on a fresh kernel, as
you press Enter or click away from it; to remove a row, empty it. An invalid
row explains why and does not run.

Explanatory links can select a syscall, move directly to a function it called,
switch a detail tab, or run a prepared alternative program. Use **reset** to
return from a try to the lesson's own program.

## 2. Select a syscall and a moment

Selecting a syscall row chooses its recording. The cursor is always in a
**run** of one task: a syscall it made, or what it did between its syscalls
("running": the idle task idling, a kernel worker writing back, jbd2
committing). A click on any task's bar in the timeline puts the cursor in that
task's run, to step through as any syscall. kSEE keeps two related units:

- A **trace record** is a raw event such as a basic block, return, store, task,
  comparison, or variable update.
- A **line step** is a reconstructed source-line position. One basic-block
  record can produce several line steps.

The chart's axis is line steps, drawn exactly (a bar thinner than a pixel,
once per pixel), however far you zoom or pan: a bar is as wide as the steps
its call takes, and stepping (← →) moves the cursor one step across it.

### Timeline

The narrow overview covers the whole snapshot. It shows which task ran, where
syscalls begin, the visible window, and the cursor. Click it to move the window.

The main chart stacks calls over time, with the outermost call at the bottom.
A bar is an instrumented call or inlined code, colored by the part of the kernel
its source is in (the legend), a concept a color family: storage green (`fs/`,
`block/`), the core kernel warm (`mm/` gold, `kernel/` orange, the scheduler
coral: where a task sleeps and another runs), the machine blue (`drivers/`,
the port's `arch/` fainter), and the helpers a cool gray (`lib/`,
`include/`); a file in none of them, gray. Hover the chart to see that
moment everywhere (the bar's call in a tip); click to keep the cursor there.
The task strip below the chart shows which task ran in each region
(the idle task gray). The cursor is a cross: down, its moment; across,
under the frame you read.

The chart opens on the whole recording, and only you zoom: choosing a line of
the program, following a link or stepping moves the cursor, and the window
slides along to keep it in view at the same zoom (from far away, centered on
it). **zoom to** shows the call you read whole (so does a double-click on a
bar); **zoom out**, the whole recording again.

### Zoom and pan

- Turn the wheel vertically to zoom around the pointer.
- Scroll horizontally, or hold Shift while using a vertical wheel, to pan.
- Drag the chart to pan directly.
- Click the overview where the window, as wide as it is, should move.

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
keeps the current line in view; a long line scrolls sideways, the line numbers
staying. The current function is edged, and lines that ran in the selected
syscall are marked. Hover a line number to see how often it ran.

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

### Calls

The Calls tab shows the whole syscall at the cursor as a tree: what it called
and what those called, with the code inlined into each where it is (`·
inlined`). Each function appears once per path of callers, with how often it
was called there (`×2`). The frames the cursor is in are highlighted, the
innermost in bold. Select a function to move the cursor to where it is first
reached; fold a subtree with its arrow.

## 4. Inspect tasks, objects, values, and output

### Tasks

The Tasks tab lists every task at the cursor, newest first (the program's,
then the kernel's own): which is running, and the others runnable, sleeping,
exited, or not yet created, each with what it is doing.

Click a task (the running one, at first) for its `task_struct` under its line,
as memory was at that moment: its key fields, as paths in it: who it is
(`pid`, `tgid`, `comm`), its state (`__state`) and `exit_code`; its directory
(`fs->pwd.dentry`), its open files by number (`files->fd_array`), and
`bio_list`, set while it submits disk requests. Check **all fields** for the whole struct instead, as declared.

Everywhere a value is shown, a pointer to a task, an open file or a dentry
says what it is: `→ init(1)`, `→ /dev/console`, `→ pipefs:[12]` (a pipe,
as Linux names it). The same address is the same object. A run of three or
more of the same plain value in an array is one row (`[4…63] NULL`).

### Memory

The Memory tab summarizes what the call you read (the frame chosen on the
Stack or the chart: the innermost, unless you choose another) wrote in kernel
memory, with everything it called; for the whole syscall, choose its
outermost frame (Down, to the bottom). It groups changes made by the call's
own task, changes made by other tasks while it ran, and values changed and
restored before the end. Each object is
named from typed kernel data and arranged under an object that points to it.

A member row shows its before and after values and the function that changed
it. Click the row to move just after that write (the next line, its value
written). The tab intentionally omits a call's ordinary stack locals; those
belong in Stack.

### Disk

The Disk tab is the disk's history (`/dev/wda`, the host's): every transfer
it made, in the order the driver handed them over (the disk makes them first
in, first out, at the CPU's next idle; no scheduler reorders them), for
whichever task (the program's, a worker's writeback,
jbd2's journal). A row says read or write, its size and where on the disk, in KB
(1024 bytes), and by which task (the one running as the driver handed it over:
the program for its reads, a worker for writeback, jbd2 for the journal); a
device's view, what the bytes are is the file system's. Its tooltip has the
exact bytes, the sector and the request's tag (how the disk says which request
it did). At the
cursor's moment, transfers made before it are plain, the one under way is
marked and kept in view, and those not handed over yet are gray. Click a row
(anywhere on it) to go just after the driver handed it to the disk, in the
task that did, the block layer's calls below on its stack; "done" to go
where the kernel took it as done, in the disk's interrupt (a read's bytes in
memory then). The host notes each moment as it happens: the trace's position
when the driver calls its `disk_submit`, and when the interrupt takes the
request's tag (`disk_done`). The transfers handed over during the calls are
listed: the boot's, and the kernel's own between calls (writeback), are not. A write shows the bytes it changed, a read the bytes it read.

### Console

The Console tab contains kernel log messages and program output accumulated
for the current lesson run. The console is output-only.

## 5. Move through the recording

You read one function at a time: the frame the source shows, highlighted in
the Stack and named in the transport below the chart (the innermost, unless
you choose another: a Stack row, a bar in the chart, or Up and Down). Left
and right move through its lines, over what it calls; when it returns, on in
its caller. Up and down move along the stack, as the Stack and the chart draw
it: up, into the call a line makes (with none, on to the very next line, so
Up alone follows every line); down, out to its caller. Each step goes somewhere else: a
line the recording stops at more than once in a row (before each function
inlined into it) counts once.

| Control | Action |
|---|---|
| Right, or → Next | The line that runs next in the function you read, over what it calls; past its end, on in its caller. |
| Left, or ← Back | Back in time, not up the file, the way the last step came: after Next, the line that ran before in the function you read (in a loop, its turn before); after In, the very line before, wherever it is. |
| Up, or ↑ In | The very next line, wherever it is: into the call this line makes (its first line), else on; with a caller's frame chosen, the frame above it. |
| Down, or ↓ Out | Out to the caller, at its call (Next then goes on there). |
| Hover the chart | See the moment there everywhere (the source, the Stack, the tabs), the bar's call in a tip; leave, and the page is back at the cursor. |
| Click the chart | Pin the cursor to the moment hovered. |

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
