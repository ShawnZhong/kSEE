# Lesson authoring

Lessons live as JSON files in `user/lessons/`. The file stem is the lesson
name used in URLs and commands. The build reads each file the course lists,
adds its name and chapter, and writes the ordered collection to
`target/site/lessons.json`.

## Lesson fields

A source lesson contains:

| Field | Meaning |
|---|---|
| `title` | The lesson heading. |
| `question` | The short question below the heading. |
| `text` | Explanatory paragraphs with inline code and links into the recording. |
| `program` | The lesson's program, in chronological order. |
| `tries` | Optional alternative programs linked from the explanation. |

`name` and `chapter` are generated during the build; do not repeat them in the
source file.

A minimal lesson looks like this:

```json
{
  "title": "What happens in read()?",
  "question": "Which parts of the kernel does a small read go through?",
  "text": [
    "Open the file, then follow [vfs_read](2:vfs_read).",
    "Try [reading twice](try:1)."
  ],
  "tries": [
    [
      "char buf[64]",
      "int fd = openat(AT_FDCWD, \"/note.txt\", O_RDONLY)",
      "read(fd, buf, 64)",
      "read(fd, buf, 64)"
    ]
  ],
  "program": [
    "char buf[64]",
    "int fd = openat(AT_FDCWD, \"/note.txt\", O_RDONLY)",
    "read(fd, buf, 64)"
  ]
}
```

## Links into a recording

Lesson paragraphs use Markdown-like inline code and links parsed by
`ksee-core`:

| Target | Result |
|---|---|
| `[the call](2)` | Select the second syscall row. |
| `[vfs_read](2:vfs_read)` | Select call 2 and move to the first `vfs_read` on that call's task. |
| `[wake it](2:try_to_wake_up:any)` | Find the function on any task that ran during call 2. |
| `[changed data](2:memory)` | Select call 2 and open Memory. |
| `[the stack](2:vfs_read:stack)` | Move into the function and open Stack. |
| `[wake stack](2:try_to_wake_up:any:stack)` | Find the function on any task and open Stack. |
| `[try it](try:1)` | Run `tries[0]` instead of the lesson program. |

Call and try numbers are one-based and must be positive. The reserved tab
suffixes are `stack`, `memory`, `tasks`, `disk`, and `console`; `any` is valid only
after a function name. Call numbers count syscall lines only, so declarations
do not increment them. Malformed targets fail lesson self-test validation.
A try keeps the lesson prose visible, but recording links are disabled because
the alternative program may have different calls.

Use backticks for inline kernel names, paths, values, and code. Keep each
paragraph focused on what the linked point demonstrates.

## Program lines and task columns

A line for the main program is a JSON string:

```json
"write(fd, \"hello\\n\", 6)"
```

A line for a secondary task is an object:

```json
{
  "task": "child",
  "text": "read(fds[0], buf, 16)"
}
```

The task name must be the named result of an earlier call that created an
active task, such as:

```text
pid_t child = clone(CLONE_VM | SIGCHLD, mem + 65536, NULL, NULL, 0)
```

The UI places secondary tasks' lines in task columns. A blocked call remains pending
until a later line wakes it. Task selection is structured JSON metadata, not
part of the statement text.

## Program language

Each line either declares program memory or makes a syscall, optionally giving
its result a typed name:

```text
char buf[64]
int fd = openat(AT_FDCWD, "/note.txt", O_RDONLY)
getpid()
```

A line sees constants and names produced by earlier lines. Calls use the
kernel's packed syscall table, parameter types, UAPI constants, and error
names, so the accepted surface follows the built kernel.

### Expressions

The expression language is deliberately C-like and small:

- decimal, hexadecimal, and octal integers;
- kernel constants such as `AT_FDCWD`, `O_CREAT`, and `NULL`;
- unary `-` and `~`;
- `|`, `^`, `&`, `<<`, `>>`, `+`, `-`, `*`, and `/`;
- earlier results and variables;
- address-of;
- array indexing and pointer arithmetic;
- strings with C escapes; and
- positional compound values.

Examples:

```text
O_CREAT | O_WRONLY
0644
{0, 30000000}
{"init", NULL}
fds[1]
&ts
```

Compound values are laid out according to the syscall parameter or declared
kernel type. String and compound syscall arguments are stored in the runner's
program memory for the lifetime of that lesson run.

Declarations can initialize scalars, arrays, and structs. A pointer is usually
a syscall result or the address of declared memory rather than a standalone
initialized pointer declaration. Invalid types, syscalls, argument counts, or
expressions are rejected before the kernel runs.

## Course order

The generated course order is defined by `COURSE` in
`crates/ksee-cli/src/lessons.rs`:

1. Start here
2. A process
3. Files, in memory
4. Files on a disk (ext4)
5. Waiting and waking
6. Your own

Names listed in `COURSE` appear in that order; a listed file that is missing
fails the build, and a JSON file not listed is left out.

## Filesystem fixtures

Each fresh kernel gets two fixture areas:

- The in-memory root filesystem contains `/note.txt` (`user/note.txt`).
- `/init` mounts an ext4 image at `/mnt`: the files under `user/disk/`, such
  as `/mnt/note.txt` and `/mnt/dir/a.txt`.

The build (the Makefile) creates the ext4 image of `user/disk/` with
`mkfs.ext4` and places it in the initramfs as `/disk.img`; add a file there to
give the lessons another. Lessons should use these shared fixtures or create
what they need in earlier lines. Opening another lesson starts from the
original fixtures again.

## Preview and validate

Build and serve the site while editing:

```sh
./ksee.sh
```

Run the kernel/browser lesson checks before committing a new lesson:

```sh
./ksee.sh test
```

Add `--ui` when the change depends on page interactions. The complete command
and test reference is in [development.md](development.md).
