# Builtin Shell Commands

The Crabv6 kernel includes an interactive shell with several builtin commands implemented directly in kernel space.

## Overview

All builtin commands are implemented in `src/main.rs:120-280` as part of the main shell loop. Commands are executed synchronously and run with full kernel privileges.

## Command Reference

### `echo`

**Syntax**: `echo <text...>`

**Description**: Prints the provided text to the console.

**Implementation**: `src/main.rs:120-123`

```rust
command if command.starts_with("echo") => {
    let output: Vec<_> = command.split_ascii_whitespace().skip(1).collect();
    println!("{}", output.join(" "));
}
```

**Details**:
- Splits command on whitespace
- Skips first token (the "echo" command itself)
- Joins remaining tokens with spaces
- No escape sequences or variable expansion

**Examples**:
```
/> echo Hello, world!
Hello, world!

/> echo Multiple    spaces    preserved
Multiple spaces preserved
```

**Limitations**:
- Multiple spaces are collapsed to single spaces
- No quoting support
- No escape sequences (\n, \t, etc.)
- No variable expansion

---

### `help`, `?`, `h`

**Syntax**: `help` | `?` | `h`

**Description**: Displays available commands and usage information.

**Implementation**: `src/main.rs:113-118`

**Output**:
```
Available commands:
  echo <text>             - Print text to console
  help, ?, h              - Show this help message
  clear                   - Clear the screen
  shutdown, sd, exit      - Shutdown the system
  fs <subcommand> [args]  - Filesystem operations
  run <path> [args...]    - Execute a user program
```

---

### `clear`

**Syntax**: `clear`

**Description**: Clears the terminal screen.

**Implementation**: `src/main.rs:125-127`

```rust
"clear" => {
    print!("\x1b[2J\x1b[H");
}
```

**Details**:
- Uses ANSI escape sequences
- `\x1b[2J` - Clear entire screen
- `\x1b[H` - Move cursor to home position (1,1)

---

### `shutdown`, `sd`, `exit`

**Syntax**: `shutdown` | `sd` | `exit`

**Description**: Shuts down the system by calling the SBI shutdown function.

**Implementation**: `src/main.rs:129`

```rust
"shutdown" | "sd" | "exit" => {
    sbi::shutdown();
}
```

**Details**:
- Uses RISC-V SBI (Supervisor Binary Interface) shutdown call
- Cleanly terminates QEMU
- No filesystem sync or cleanup (TinyFS doesn't require it)

---

## Filesystem Commands

All filesystem commands begin with the `fs` prefix and are implemented in `handle_fs_command()` at `src/main.rs:290-442`.

### `fs format`

**Syntax**: `fs format`

**Description**: Formats the filesystem, erasing all data and creating a fresh root directory.

**Implementation**: `src/main.rs:316-322`

```rust
"format" => {
    FS.lock().format()?;
    println!("Filesystem formatted successfully");
}
```

**Details**:
- Initializes superblock with magic number and version
- Creates empty root directory
- Resets allocation bitmap
- **WARNING**: Destroys all existing data

---

### `fs ls`

**Syntax**: `fs ls [path]`

**Description**: Lists the contents of a directory.

**Implementation**: `src/main.rs:324-361`

**Default**: If no path is provided, lists current working directory.

**Output Format**:
```
<name>  <type>  <size_or_block>
```
- **name**: Entry name
- **type**: "DIR" or "FILE"
- **size_or_block**: Size in bytes for files, block number for directories

**Examples**:
```
/> fs ls
bin         DIR     2
test        DIR     3

/> fs ls /bin
cat2        FILE    1234
```

---

### `fs cat`

**Syntax**: `fs cat <path>`

**Description**: Reads and displays the contents of a file.

**Implementation**: `src/main.rs:363-383`

```rust
"cat" => {
    let path = /* parse path */;
    let data = FS.lock().read_file(path)?;

    match core::str::from_utf8(&data) {
        Ok(s) => print!("{}", s),
        Err(_) => println!("(binary data, {} bytes)", data.len()),
    }
}
```

**Details**:
- Reads entire file into memory
- Attempts UTF-8 decode
- If UTF-8, displays as text
- If binary, shows size only

**Examples**:
```
/> fs cat /test/hello.txt
Hello, world!

/> fs cat /bin/cat2
(binary data, 4096 bytes)
```

---

### `fs write`

**Syntax**: `fs write <path> <text>`

**Description**: Writes text to a file, creating it if it doesn't exist.

**Implementation**: `src/main.rs:385-402`

**Details**:
- Takes entire remaining command line as text content
- Creates file if it doesn't exist
- Overwrites file if it exists (doesn't append)
- File must be opened in parent directory

**Examples**:
```
/> fs write /test/hello.txt Hello, world!
File written successfully

/> fs write /test/multiword.txt This is a longer sentence
File written successfully
```

**Limitations**:
- No quoting mechanism
- Cannot write to files in non-existent directories (create directory first)
- Entire file is replaced on write (no append mode)

---

### `fs cd`

**Syntax**: `fs cd <path>`

**Description**: Changes the current working directory.

**Implementation**: `src/main.rs:404-414`

**Details**:
- Supports absolute paths (starting with `/`)
- Supports relative paths
- Parent directory with `..`
- Validates that target exists and is a directory

**Examples**:
```
/> fs cd /test
/test> fs cd ..
/> fs cd test
/test>
```

**Special Paths**:
- `/` - Root directory
- `..` - Parent directory
- `.` - Current directory (supported but no-op)

---

### `fs mkdir`

**Syntax**: `fs mkdir <path>`

**Description**: Creates a new directory.

**Implementation**: `src/main.rs:416-425`

**Details**:
- Creates directory in specified path
- Parent directory must exist
- Cannot create nested directories in one command

**Examples**:
```
/> fs mkdir /test
Directory created

/> fs mkdir /test/subdir
Directory created
```

**Errors**:
- `ENOENT` - Parent directory doesn't exist
- `EEXIST` - Directory already exists
- `ENOSPC` - No space left on device

---

### `fs rm`

**Syntax**: `fs rm <path>`

**Description**: Removes a file.

**Implementation**: `src/main.rs:427-436`

**Details**:
- Only removes files, not directories (use `fs rmdir` for directories)
- Cannot remove non-empty directories
- File is immediately deleted (no trash/recycle bin)

**Examples**:
```
/> fs rm /test/hello.txt
File removed

/> fs rm /test
Error: Is a directory
```

---

### `fs rmdir`

**Syntax**: `fs rmdir <path>` (not explicitly shown but implied by implementation)

**Description**: Removes an empty directory.

**Details**:
- Directory must be empty
- Cannot remove root directory
- Returns `ENOTEMPTY` if directory has entries

---

## Program Execution

### `run`

**Syntax**: `run <path> [args...]`

**Description**: Loads and executes an ELF binary from the filesystem.

**Implementation**: `src/main.rs:141-180`

**Process**:
1. Parse command into program path and arguments
2. Read ELF binary from filesystem
3. Load ELF into user memory window
4. Set up stack with argc/argv
5. Jump to user mode
6. Wait for program to exit
7. Display exit code

**Details**:
- Supports up to 16 command-line arguments
- Arguments are null-terminated C strings on user stack
- Exit code is displayed after program completes
- If exit code is non-zero, "Process exited with code N" is shown

**Examples**:
```
/> run /bin/cat2 /test/hello.txt
Hello, world!
Hello, world!
Process exited with code 0

/> run /bin/nonexistent
Error: File not found
```

**Argument Passing**:
```
/> run /bin/cat2 file1.txt file2.txt
```
Results in:
- argc = 3
- argv[0] = "/bin/cat2"
- argv[1] = "file1.txt"
- argv[2] = "file2.txt"

---

## Command Parsing

**Implementation**: `src/main.rs:284-288`

Commands are parsed by:
1. Trimming whitespace
2. Checking for empty input (ignored)
3. Pattern matching on command prefix
4. Splitting on whitespace for arguments

**Limitations**:
- No quoting mechanism (cannot have spaces in arguments)
- No escape sequences
- No command history or editing (use QEMU console features)
- No tab completion
- No wildcards or globbing

---

## Error Handling

All commands that interact with the filesystem return `Result<(), FsError>`. Errors are displayed to the user with descriptive messages:

**Common Errors**:
- `ENOENT` (-2) - "File not found" or "No such file or directory"
- `EINVAL` (-22) - "Invalid argument"
- `ENOSPC` (-28) - "No space left on device"
- `ENOTEMPTY` (-39) - "Directory not empty"

**Example**:
```
/> fs cat /nonexistent.txt
Error: No such file or directory (errno -2)
```

---

## Adding New Builtin Commands

To add a new builtin command:

1. Add pattern match in `src/main.rs` main loop:
```rust
"mycommand" => {
    // Implementation
}
```

2. Update help text in `help` command

3. Add error handling as needed

4. Keep implementation simple and kernel-safe (no panics on user input)

---

## Future Enhancements

Potential improvements to the shell:

- [ ] Command history (up/down arrows)
- [ ] Tab completion for paths
- [ ] Quoting and escape sequences
- [ ] Wildcards and globbing
- [ ] Command piping (`|`)
- [ ] Output redirection (`>`, `>>`)
- [ ] Environment variables
- [ ] Aliases
- [ ] Background processes (`&`)
- [ ] Job control

These are intentionally not implemented to keep the codebase simple and focused on core OS concepts.
