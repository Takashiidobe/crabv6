# Crabv6

Writing Xv6 in Rust.

## Day 1

Following https://blog.henrygressmann.de/rust-os/2-shell/. Some changes
including swapping out the allocator for `linked_list_allocator`.

## Bugs

- multiple CPUs spinning to 100% on start. Need to use `wfi` somehow in
  the main loop?

## Initializing the file system

Create a file system image to use if it doesn't exist already.

```sh
dd if=/dev/zero of=disk.img bs=1M count=16
```
