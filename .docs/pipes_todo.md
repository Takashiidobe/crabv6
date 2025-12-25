# Pipes and Redirection: Next Steps

This plan tracks what is left to make pipes and redirection behave like xv6.

## Reality Check (current code)
- `src/fd.rs` pipes return `WouldBlock` but no wakeups occur, so readers/writers can block forever.
- `FdTable::close` just drops entries; pipe ends are never marked closed, so EOF/EPIPE never surface.
- Kernel shell `handle_pipe` is stubbed and redirection happens in the kernel shell instead of in child processes. Pipelines never run concurrently there.

## Target Behavior (xv6-inspired)
- Shared pipe object with refcounted ends; closing updates `read_end_open/write_end_open` and frees when both ends are gone.
- Blocking semantics: empty read blocks until data or writer closes (then returns 0), full write blocks until space or reader closes (then returns EPIPE/BrokenPipe).
- Wakeup paths: reads wake writers, writes wake readers; close wakes the opposite end.
- Pipe/redirection wiring happens in user-space shell right before `exec` of each stage, not in the kernel shell.

## TODO
- [x] Add pipe end lifecycle tracking: `FdTable::close` now notifies `close_pipe_end`, and pipe refcounts are bumped on dup/clone.
- [x] Implement per-pipe blocked PID lists so empty/full pipes block and are woken by the opposite end.
- [x] Wake on close: when read end closes, writers are unblocked and see `BrokenPipe`; when write end closes and the buffer drains, readers get EOF.
- [x] Move pipeline/redirection setup into the user shell (kernel shell now defers to `/bin/sh`).
- [x] Close all FDs on process exit so pipe ends get closed and readers see EOF.
- [x] Block on pipe I/O by returning EAGAIN to user and retrying in user_bin helpers; scheduler switches on the blocked state.
- [ ] Re-test basic redirection (`<`, `>`, `>>`) and simple pipelines (`cat file | wc`) from user shell.
