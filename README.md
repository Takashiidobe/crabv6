# Crabv6

Writing Xv6 in Rust.

## Day 1

Following https://blog.henrygressmann.de/rust-os/2-shell/. Some changes
including swapping out the allocator for `linked_list_allocator`.

## Notes

- Secondary harts now use `wfi` to idle instead of busy spinning, so
  only the primary hart remains active while waiting for input.

## Initializing the file system

Create a file system image to use if it does not exist already:

```sh
dd if=/dev/zero of=disk.img bs=1M count=16
```
