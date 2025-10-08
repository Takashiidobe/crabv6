# Crabv6

Writing Xv6 in Rust.

## Day 1

Following https://blog.henrygressmann.de/rust-os/2-shell/. Some changes
including swapping out the allocator for `linked_list_allocator`.

## Notes

- Secondary harts now use `wfi` to idle instead of busy spinning, and
  hart 0 sleeps until the UART raises a PLIC interrupt, eliminating the
  constant 100% CPU load.

## Initializing the file system

Create a file system image to use if it does not exist already:

```sh
dd if=/dev/zero of=disk.img bs=1M count=16
```

Inside the shell you can use `fs` commands (mkdir, write, ls, cd, cat, format) to manage the disk. Use `run <path>` to load an ELF binary and jump to user mode.
