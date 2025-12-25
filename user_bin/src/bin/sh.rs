#![no_std]
#![no_main]

use core::str;
use user_bin::{
    close, dup2, exit, open, pipe, read, spawn, wait, write, O_APPEND, O_CREATE, O_READ,
    O_WRITE,
};

const MAX_LINE: usize = 256;
const MAX_ARGS: usize = 8;
const PROMPT: &[u8] = b"sh> ";

struct Redir<'a> {
    path: &'a str,
    append: bool,
}

struct Command<'a> {
    args: [&'a str; MAX_ARGS],
    argc: usize,
    stdin: Option<&'a str>,
    stdout: Option<Redir<'a>>,
}

impl<'a> Command<'a> {
    const fn new() -> Self {
        Self {
            args: [""; MAX_ARGS],
            argc: 0,
            stdin: None,
            stdout: None,
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn _start(_argc: usize, _argv: *const *const u8) -> ! {
    let mut line_buf = [0u8; MAX_LINE];

    loop {
        write(1, PROMPT);
        let line_len = read_line(&mut line_buf);
        if line_len == 0 {
            continue;
        }

        let line = match str::from_utf8(&line_buf[..line_len]) {
            Ok(s) => s.trim(),
            Err(_) => {
                write(2, b"invalid utf-8 input\n");
                continue;
            }
        };

        if line.is_empty() {
            continue;
        }
        if line == "exit" {
            exit(0);
        }

        let mut cmds = [Command::new(), Command::new(), Command::new(), Command::new(), Command::new(), Command::new(), Command::new(), Command::new()];
        let parsed = match parse_commands(line, &mut cmds) {
            Ok(n) => n,
            Err(msg) => {
                write(2, msg.as_bytes());
                write(2, b"\n");
                continue;
            }
        };

        if let Err(msg) = run_pipeline(&cmds[..parsed]) {
            write(2, msg.as_bytes());
            write(2, b"\n");
        }
    }
}

fn read_line(buf: &mut [u8]) -> usize {
    let mut idx = 0;
    let mut byte_buf = [0u8; 1];

    loop {
        let n = read(0, &mut byte_buf);
        if n <= 0 {
            continue;
        }
        let b = byte_buf[0];
        if b == b'\r' || b == b'\n' {
            write(1, b"\n");
            break;
        }
        if b == 0x08 || b == 0x7f {
            if idx > 0 {
                idx -= 1;
                write(1, b"\x08 \x08");
            }
            continue;
        }
        if idx < buf.len() {
            buf[idx] = b;
            idx += 1;
            write(1, &byte_buf);
        }
    }
    idx
}

fn parse_commands<'a>(line: &'a str, cmds: &mut [Command<'a>]) -> Result<usize, &'static str> {
    let mut cmd_idx = 0;
    let mut cur = Command::new();
    let bytes = line.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        while i < bytes.len() && is_space(bytes[i]) {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }

        match bytes[i] {
            b'|' => {
                if cur.argc == 0 {
                    return Err("syntax error: empty command before |");
                }
                if cmd_idx >= cmds.len() {
                    return Err("too many pipeline stages");
                }
                cmds[cmd_idx] = cur;
                cmd_idx += 1;
                cur = Command::new();
                i += 1;
            }
            b'<' => {
                i += 1;
                while i < bytes.len() && is_space(bytes[i]) {
                    i += 1;
                }
                let (token, next) = parse_token(line, i)?;
                cur.stdin = Some(token);
                i = next;
            }
            b'>' => {
                let mut append = false;
                if i + 1 < bytes.len() && bytes[i + 1] == b'>' {
                    append = true;
                    i += 2;
                } else {
                    i += 1;
                }
                while i < bytes.len() && is_space(bytes[i]) {
                    i += 1;
                }
                let (token, next) = parse_token(line, i)?;
                cur.stdout = Some(Redir { path: token, append });
                i = next;
            }
            _ => {
                let (token, next) = parse_token(line, i)?;
                if cur.argc >= MAX_ARGS {
                    return Err("too many args");
                }
                cur.args[cur.argc] = token;
                cur.argc += 1;
                i = next;
            }
        }
    }

    if cur.argc == 0 {
        return Err("empty command");
    }
    if cmd_idx >= cmds.len() {
        return Err("too many pipeline stages");
    }
    cmds[cmd_idx] = cur;
    Ok(cmd_idx + 1)
}

fn parse_token<'a>(line: &'a str, start: usize) -> Result<(&'a str, usize), &'static str> {
    let bytes = line.as_bytes();
    let mut end = start;
    while end < bytes.len() && !is_space(bytes[end]) && bytes[end] != b'|' && bytes[end] != b'<' && bytes[end] != b'>' {
        end += 1;
    }
    if end == start {
        return Err("expected token");
    }
    let token = &line[start..end];
    Ok((token, end))
}

fn is_space(b: u8) -> bool {
    b == b' ' || b == b'\t'
}

fn run_pipeline(cmds: &[Command]) -> Result<(), &'static str> {
    if cmds.is_empty() {
        return Err("empty pipeline");
    }

    let mut pids: [isize; 8] = [-1; 8];
    let mut stdin_fd: isize = -1; // fd for next command's stdin

    for (idx, cmd) in cmds.iter().enumerate() {
        let is_last = idx + 1 == cmds.len();
        write(2, b"[pipeline] processing cmd\n");

        // Determine stdin for this command
        let cmd_stdin_fd = if let Some(path) = cmd.stdin {
            // Explicit input redirection
            let fd = open(path, O_READ);
            if fd < 0 {
                cleanup_pipeline(idx, &pids);
                return Err("failed to open stdin redirection");
            }
            fd
        } else {
            stdin_fd
        };

        // Determine stdout for this command
        let (cmd_stdout_fd, pipe_read_fd) = if is_last {
            // Last command - use explicit redirection or stdout
            if let Some(redir) = cmd.stdout.as_ref() {
                let mut flags = O_WRITE | O_CREATE;
                if redir.append {
                    flags |= O_APPEND;
                }
                let fd = open(redir.path, flags);
                if fd < 0 {
                    if cmd_stdin_fd >= 0 {
                        close(cmd_stdin_fd as usize);
                    }
                    cleanup_pipeline(idx, &pids);
                    return Err("failed to open stdout redirection");
                }
                (fd, -1)
            } else {
                (-1, -1) // Use default stdout
            }
        } else {
            // Not last - create pipe for next command
            let mut pipe_fds = [0usize; 2];
            if pipe(&mut pipe_fds) < 0 {
                if cmd_stdin_fd >= 0 {
                    close(cmd_stdin_fd as usize);
                }
                cleanup_pipeline(idx, &pids);
                return Err("failed to create pipe");
            }
            (pipe_fds[1] as isize, pipe_fds[0] as isize)
        };

        // Spawn command
        write(2, b"[pipeline] about to spawn cmd\n");
        let pid = spawn_command(cmd, cmd_stdin_fd, cmd_stdout_fd)?;
        write(2, b"[pipeline] spawned cmd\n");
        if pid < 0 {
            if cmd_stdin_fd >= 0 {
                close(cmd_stdin_fd as usize);
            }
            if cmd_stdout_fd >= 0 {
                close(cmd_stdout_fd as usize);
            }
            if pipe_read_fd >= 0 {
                close(pipe_read_fd as usize);
            }
            cleanup_pipeline(idx, &pids);
            return Err("failed to spawn command");
        }
        pids[idx] = pid;

        // Close used fds in parent
        write(2, b"[pipeline] closing parent fds\n");
        if cmd_stdin_fd >= 0 {
            close(cmd_stdin_fd as usize);
        }
        if cmd_stdout_fd >= 0 {
            close(cmd_stdout_fd as usize);
        }

        // Pipe read end becomes stdin for next command
        stdin_fd = pipe_read_fd;
        write(2, b"[pipeline] done with cmd\n");
    }

    write(2, b"[pipeline] all commands spawned, waiting...\n");

    // Wait for all children
    for i in 0..cmds.len() {
        if pids[i] >= 0 {
            wait(None);
        }
    }

    Ok(())
}

// Cleanup any spawned processes
fn cleanup_pipeline(up_to: usize, pids: &[isize; 8]) {
    for i in 0..up_to {
        if pids[i] >= 0 {
            // Processes will be cleaned up when parent waits or exits
        }
    }
}

// Spawn a command with specified stdin/stdout file descriptors
// stdin_fd: -1 means use default stdin, otherwise dup2 to stdin
// stdout_fd: -1 means use default stdout, otherwise dup2 to stdout
// Returns the child PID or negative error code
fn spawn_command(cmd: &Command, stdin_fd: isize, stdout_fd: isize) -> Result<isize, &'static str> {
    if cmd.argc == 0 {
        return Err("empty command");
    }

    write(2, b"[spawn_command] start\n");

    // Save current stdin/stdout
    let mut saved_in = dup2(0, 14);
    if saved_in < 0 {
        saved_in = -1;
    }
    let mut saved_out = dup2(1, 15);
    if saved_out < 0 {
        saved_out = -1;
    }

    write(2, b"[spawn_command] saved stdio\n");

    // Redirect stdin if needed
    if stdin_fd >= 0 {
        dup2(stdin_fd as usize, 0);
    }

    write(2, b"[spawn_command] redirected stdin\n");

    // Redirect stdout if needed
    if stdout_fd >= 0 {
        dup2(stdout_fd as usize, 1);
    }

    write(2, b"[spawn_command] redirected stdout\n");

    // Build argv
    let mut argv_buf: [&str; 16] = [""; 16];
    let argc = cmd.argc.min(16);
    for i in 0..argc {
        argv_buf[i] = cmd.args[i];
    }

    // Resolve program path
    let mut path_buf = [0u8; MAX_LINE];
    let prog_path = resolve_prog(cmd.args[0], &mut path_buf);

    write(2, b"[spawn_command] about to spawn\n");
    write(2, b"[spawn_command] prog_path=");
    write(2, prog_path.as_bytes());
    write(2, b"\n");
    write(2, b"[spawn_command] argc=");
    let argc_byte = [b'0' + (argc as u8)];
    write(2, &argc_byte);
    write(2, b"\n");
    for i in 0..argc {
        write(2, b"[spawn_command] argv[");
        let i_byte = [b'0' + (i as u8)];
        write(2, &i_byte);
        write(2, b"]=\"");
        write(2, argv_buf[i].as_bytes());
        write(2, b"\"\n");
    }

    // Spawn child
    let pid = spawn(prog_path, &argv_buf[..argc]);

    write(2, b"[spawn_command] spawn returned\n");

    // Restore parent's stdin/stdout
    restore_stdio(saved_in, saved_out);

    write(2, b"[spawn_command] restored stdio\n");

    if pid < 0 {
        return Err("spawn failed");
    }

    write(2, b"[spawn_command] done\n");

    Ok(pid)
}

fn restore_stdio(saved_in: isize, saved_out: isize) {
    if saved_in >= 0 {
        dup2(saved_in as usize, 0);
        close(saved_in as usize);
    }
    if saved_out >= 0 {
        dup2(saved_out as usize, 1);
        close(saved_out as usize);
    }
}

fn resolve_prog<'a>(cmd: &'a str, buf: &'a mut [u8; MAX_LINE]) -> &'a str {
    if cmd.starts_with('/') {
        return cmd;
    }

    const PREFIX: &str = "/bin/";
    let prefix_bytes = PREFIX.as_bytes();
    let cmd_bytes = cmd.as_bytes();
    let total = prefix_bytes.len() + cmd_bytes.len();
    if total >= buf.len() {
        return cmd;
    }

    buf[..prefix_bytes.len()].copy_from_slice(prefix_bytes);
    buf[prefix_bytes.len()..total].copy_from_slice(cmd_bytes);
    str::from_utf8(&buf[..total]).unwrap_or(cmd)
}
