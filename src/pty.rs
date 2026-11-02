//! A program in a pseudo-terminal: ConPTY on Windows, `openpty` on macOS, through the system's
//! own functions. What the program draws comes out as bytes; keys go in as bytes. System specific code
//! stays here.

use std::io;
use std::path::{Path, PathBuf};

/// A running program in its pseudo-terminal. Dropping it closes the terminal, which ends the program.
pub struct Pty(imp::Pty);

/// What the program draws; ends once the program has ended and the terminal is closed.
pub struct Output(imp::Output);

impl io::Read for Output {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }
}

impl Pty {
    /// Starts `program` (a name looked up in PATH, or a path) with `args` in `cwd`, with exactly `env`.
    pub fn spawn(program: &str, args: &[String], cwd: &str, env: &[(String, String)], cols: u16, rows: u16) -> io::Result<(Pty, Output)> {
        let path = find(program, env).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("{program} is not in PATH")))?;
        let (pty, out) = imp::spawn(&path, args, cwd, env, cols.max(2), rows.max(2))?;
        Ok((Pty(pty), Output(out)))
    }

    pub fn write(&self, data: &[u8]) -> io::Result<()> {
        self.0.write(data)
    }

    pub fn resize(&self, cols: u16, rows: u16) -> io::Result<()> {
        self.0.resize(cols.max(2), rows.max(2))
    }

    pub fn pid(&self) -> u32 {
        self.0.pid()
    }

    /// Waits for the program to end, then closes the terminal so that `Output` ends.
    pub fn wait(&self) {
        self.0.wait();
        self.0.close();
    }
}

/// The program's full path, looked up in the PATH given to it.
fn find(program: &str, env: &[(String, String)]) -> Option<PathBuf> {
    if Path::new(program).components().count() > 1 {
        return Path::new(program).is_file().then(|| PathBuf::from(program));
    }
    let path = env.iter().find(|(k, _)| k.eq_ignore_ascii_case("PATH")).map(|(_, v)| v.clone())?;
    let exts: &[&str] = if cfg!(windows) { &[".exe", ".cmd", ".bat"] } else { &[""] };
    std::env::split_paths(&path).flat_map(|dir| exts.iter().map(move |e| dir.join(format!("{program}{e}")))).find(|p| p.is_file())
}

/// One argument of a Windows command line, quoted as `CommandLineToArgvW` reads it back.
#[cfg_attr(not(windows), allow(dead_code))]
fn win_quote(arg: &str) -> String {
    if !arg.is_empty() && !arg.contains([' ', '\t', '"']) {
        return arg.to_string();
    }
    let mut out = String::from("\"");
    let mut slashes = 0;
    for c in arg.chars() {
        match c {
            '\\' => slashes += 1,
            '"' => {
                out.push_str(&"\\".repeat(slashes * 2 + 1));
                out.push('"');
                slashes = 0;
            }
            _ => {
                out.push_str(&"\\".repeat(slashes));
                out.push(c);
                slashes = 0;
            }
        }
    }
    out.push_str(&"\\".repeat(slashes * 2));
    out.push('"');
    out
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod imp {
    use std::ffi::c_void;
    use std::io;
    use std::path::Path;
    use std::ptr::null_mut;
    use std::sync::atomic::{AtomicPtr, Ordering};

    type Handle = *mut c_void;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Coord {
        x: i16,
        y: i16,
    }

    #[repr(C)]
    struct StartupInfo {
        cb: u32,
        reserved: *mut u16,
        desktop: *mut u16,
        title: *mut u16,
        x: u32,
        y: u32,
        x_size: u32,
        y_size: u32,
        x_chars: u32,
        y_chars: u32,
        fill: u32,
        flags: u32,
        show: u16,
        reserved2_len: u16,
        reserved2: *mut u8,
        stdin: Handle,
        stdout: Handle,
        stderr: Handle,
    }

    #[repr(C)]
    struct StartupInfoEx {
        info: StartupInfo,
        attributes: *mut c_void,
    }

    #[repr(C)]
    struct ProcessInfo {
        process: Handle,
        thread: Handle,
        pid: u32,
        tid: u32,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreatePipe(read: *mut Handle, write: *mut Handle, attributes: *const c_void, size: u32) -> i32;
        fn CreatePseudoConsole(size: Coord, input: Handle, output: Handle, flags: u32, console: *mut Handle) -> i32;
        fn ResizePseudoConsole(console: Handle, size: Coord) -> i32;
        fn ClosePseudoConsole(console: Handle);
        fn InitializeProcThreadAttributeList(list: *mut c_void, count: u32, flags: u32, size: *mut usize) -> i32;
        fn UpdateProcThreadAttribute(list: *mut c_void, flags: u32, attribute: usize, value: *const c_void, size: usize, previous: *mut c_void, returned: *mut usize) -> i32;
        fn DeleteProcThreadAttributeList(list: *mut c_void);
        fn CreateProcessW(
            application: *const u16,
            command: *mut u16,
            process_attributes: *const c_void,
            thread_attributes: *const c_void,
            inherit: i32,
            flags: u32,
            environment: *const c_void,
            directory: *const u16,
            startup: *const StartupInfoEx,
            info: *mut ProcessInfo,
        ) -> i32;
        fn ReadFile(file: Handle, buffer: *mut u8, len: u32, read: *mut u32, overlapped: *mut c_void) -> i32;
        fn WriteFile(file: Handle, buffer: *const u8, len: u32, written: *mut u32, overlapped: *mut c_void) -> i32;
        fn WaitForSingleObject(handle: Handle, ms: u32) -> u32;
        fn CloseHandle(handle: Handle) -> i32;
    }

    const PSEUDOCONSOLE: usize = 0x0002_0016;
    const EXTENDED_STARTUPINFO: u32 = 0x0008_0000;
    const UNICODE_ENVIRONMENT: u32 = 0x0000_0400;
    const USE_STD_HANDLES: u32 = 0x0000_0100;
    const INFINITE: u32 = 0xFFFF_FFFF;
    const BROKEN_PIPE: i32 = 109;

    pub struct Pty {
        console: AtomicPtr<c_void>,
        input: Handle,
        process: Handle,
        pid: u32,
    }

    // SAFETY: the handles are owned by this value and the system calls on them are thread safe.
    unsafe impl Send for Pty {}
    unsafe impl Sync for Pty {}

    pub struct Output(Handle);
    // SAFETY: the pipe's read end is used by one reader at a time.
    unsafe impl Send for Output {}

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }

    fn check(ok: bool) -> io::Result<()> {
        if ok { Ok(()) } else { Err(io::Error::last_os_error()) }
    }

    pub fn spawn(program: &Path, args: &[String], cwd: &str, env: &[(String, String)], cols: u16, rows: u16) -> io::Result<(Pty, Output)> {
        let prog = program.display().to_string();
        let words: Vec<String> = std::iter::once(prog.clone()).chain(args.iter().cloned()).map(|a| super::win_quote(&a)).collect();
        // A .cmd or .bat file runs through cmd.exe.
        let line = if prog.to_ascii_lowercase().ends_with(".cmd") || prog.to_ascii_lowercase().ends_with(".bat") {
            // cmd.exe reads these itself, whatever the quoting: free text (a first instruction) cannot pass.
            if args.iter().any(|a| a.contains(['"', '%', '!', '^', '&', '|', '<', '>', '\r', '\n'])) {
                return Err(io::Error::other("claude is a .cmd here: an instruction cannot hold \" % ! ^ & | < > or a line break."));
            }
            format!("cmd.exe /d /s /c \"{}\"", words.join(" "))
        } else {
            words.join(" ")
        };
        let mut vars: Vec<&(String, String)> = env.iter().collect();
        vars.sort_by_key(|(k, _)| k.to_ascii_uppercase());
        let mut block: Vec<u16> = vars.iter().flat_map(|(k, v)| format!("{k}={v}").encode_utf16().chain(Some(0)).collect::<Vec<_>>()).collect();
        block.push(0);
        let (mut command, directory) = (wide(&line), wide(cwd));
        // SAFETY: every handle is checked and closed on each path; buffers outlive the calls using them.
        unsafe {
            let (mut in_read, mut in_write, mut out_read, mut out_write) = (null_mut(), null_mut(), null_mut(), null_mut());
            check(CreatePipe(&mut in_read, &mut in_write, std::ptr::null(), 0) != 0)?;
            if CreatePipe(&mut out_read, &mut out_write, std::ptr::null(), 0) == 0 {
                let e = io::Error::last_os_error();
                CloseHandle(in_read);
                CloseHandle(in_write);
                return Err(e);
            }
            let mut console = null_mut();
            let hr = CreatePseudoConsole(Coord { x: cols as i16, y: rows as i16 }, in_read, out_write, 0, &mut console);
            // The pseudo-console holds its own copies of these two ends.
            CloseHandle(in_read);
            CloseHandle(out_write);
            if hr < 0 {
                CloseHandle(in_write);
                CloseHandle(out_read);
                return Err(io::Error::other(format!("CreatePseudoConsole failed: {hr:#x}")));
            }
            let fail = |e: io::Error| {
                ClosePseudoConsole(console);
                CloseHandle(in_write);
                CloseHandle(out_read);
                e
            };
            let mut size = 0usize;
            InitializeProcThreadAttributeList(null_mut(), 1, 0, &mut size);
            let mut list = vec![0u8; size];
            let list_ptr = list.as_mut_ptr().cast::<c_void>();
            check(InitializeProcThreadAttributeList(list_ptr, 1, 0, &mut size) != 0).map_err(fail)?;
            if UpdateProcThreadAttribute(list_ptr, 0, PSEUDOCONSOLE, console, std::mem::size_of::<Handle>(), null_mut(), null_mut()) == 0 {
                let e = io::Error::last_os_error();
                DeleteProcThreadAttributeList(list_ptr);
                return Err(fail(e));
            }
            let mut si: StartupInfoEx = std::mem::zeroed();
            si.info.cb = std::mem::size_of::<StartupInfoEx>() as u32;
            // No standard handles of ours: the program talks to the pseudo-console only.
            si.info.flags = USE_STD_HANDLES;
            si.attributes = list_ptr;
            let mut pi: ProcessInfo = std::mem::zeroed();
            let ok = CreateProcessW(
                std::ptr::null(),
                command.as_mut_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                EXTENDED_STARTUPINFO | UNICODE_ENVIRONMENT,
                block.as_ptr().cast(),
                directory.as_ptr(),
                &si,
                &mut pi,
            );
            DeleteProcThreadAttributeList(list_ptr);
            if ok == 0 {
                return Err(fail(io::Error::last_os_error()));
            }
            CloseHandle(pi.thread);
            Ok((Pty { console: AtomicPtr::new(console), input: in_write, process: pi.process, pid: pi.pid }, Output(out_read)))
        }
    }

    impl Output {
        pub fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let mut n = 0u32;
            // SAFETY: the buffer and its length match; the handle is ours until drop.
            let ok = unsafe { ReadFile(self.0, buf.as_mut_ptr(), buf.len().min(u32::MAX as usize) as u32, &mut n, null_mut()) };
            if ok == 0 {
                let e = io::Error::last_os_error();
                return if e.raw_os_error() == Some(BROKEN_PIPE) { Ok(0) } else { Err(e) };
            }
            Ok(n as usize)
        }
    }

    impl Drop for Output {
        fn drop(&mut self) {
            // SAFETY: closed once, here.
            unsafe { CloseHandle(self.0) };
        }
    }

    impl Pty {
        pub fn write(&self, mut data: &[u8]) -> io::Result<()> {
            while !data.is_empty() {
                let mut n = 0u32;
                // SAFETY: the slice and its length match; the handle is ours until drop.
                check(unsafe { WriteFile(self.input, data.as_ptr(), data.len().min(u32::MAX as usize) as u32, &mut n, null_mut()) } != 0)?;
                data = &data[n as usize..];
            }
            Ok(())
        }

        pub fn resize(&self, cols: u16, rows: u16) -> io::Result<()> {
            let console = self.console.load(Ordering::SeqCst);
            if console.is_null() {
                return Ok(());
            }
            // SAFETY: a live pseudo-console handle.
            let hr = unsafe { ResizePseudoConsole(console, Coord { x: cols as i16, y: rows as i16 }) };
            if hr < 0 { Err(io::Error::other(format!("ResizePseudoConsole failed: {hr:#x}"))) } else { Ok(()) }
        }

        pub fn pid(&self) -> u32 {
            self.pid
        }

        pub fn wait(&self) {
            // SAFETY: the process handle is ours until drop.
            unsafe { WaitForSingleObject(self.process, INFINITE) };
        }

        /// Closes the pseudo-console once: the output pipe then ends.
        pub fn close(&self) {
            let console = self.console.swap(null_mut(), Ordering::SeqCst);
            if !console.is_null() {
                // SAFETY: swapped out, so closed once.
                unsafe { ClosePseudoConsole(console) };
            }
        }
    }

    impl Drop for Pty {
        fn drop(&mut self) {
            self.close();
            // SAFETY: closed once, here.
            unsafe {
                CloseHandle(self.input);
                CloseHandle(self.process);
            }
        }
    }
}

#[cfg(unix)]
#[allow(unsafe_code)]
mod imp {
    use std::ffi::{CString, c_char, c_int, c_void};
    use std::fs::File;
    use std::io::{self, Read, Write};
    use std::os::fd::{FromRawFd, RawFd};
    use std::path::Path;

    #[repr(C)]
    struct Winsize {
        rows: u16,
        cols: u16,
        x: u16,
        y: u16,
    }

    unsafe extern "C" {
        fn openpty(master: *mut c_int, slave: *mut c_int, name: *mut c_char, termp: *const c_void, winp: *const Winsize) -> c_int;
        fn fork() -> c_int;
        fn setsid() -> c_int;
        fn ioctl(fd: c_int, request: std::ffi::c_ulong, ...) -> c_int;
        fn dup2(from: c_int, to: c_int) -> c_int;
        fn close(fd: c_int) -> c_int;
        fn chdir(path: *const c_char) -> c_int;
        fn execve(path: *const c_char, argv: *const *const c_char, envp: *const *const c_char) -> c_int;
        fn _exit(code: c_int) -> !;
        fn waitpid(pid: c_int, status: *mut c_int, options: c_int) -> c_int;
    }

    #[cfg(target_os = "macos")]
    const TIOCSCTTY: std::ffi::c_ulong = 0x2000_7461;
    #[cfg(target_os = "macos")]
    const TIOCSWINSZ: std::ffi::c_ulong = 0x8008_7467;
    #[cfg(not(target_os = "macos"))]
    const TIOCSCTTY: std::ffi::c_ulong = 0x540E;
    #[cfg(not(target_os = "macos"))]
    const TIOCSWINSZ: std::ffi::c_ulong = 0x5414;

    pub struct Pty {
        master: File,
        pid: c_int,
    }

    pub struct Output(File);

    fn c(s: &str) -> io::Result<CString> {
        CString::new(s).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "a NUL byte in an argument"))
    }

    pub fn spawn(program: &Path, args: &[String], cwd: &str, env: &[(String, String)], cols: u16, rows: u16) -> io::Result<(Pty, Output)> {
        // Everything the child needs is built before fork: after it, only async-signal-safe calls.
        let path = c(&program.display().to_string())?;
        let argv_c: Vec<CString> = std::iter::once(program.display().to_string()).chain(args.iter().cloned()).map(|a| c(&a)).collect::<io::Result<_>>()?;
        let envp_c: Vec<CString> = env.iter().map(|(k, v)| c(&format!("{k}={v}"))).collect::<io::Result<_>>()?;
        let mut argv: Vec<*const c_char> = argv_c.iter().map(|a| a.as_ptr()).collect();
        argv.push(std::ptr::null());
        let mut envp: Vec<*const c_char> = envp_c.iter().map(|a| a.as_ptr()).collect();
        envp.push(std::ptr::null());
        let dir = c(cwd)?;
        let size = Winsize { rows, cols, x: 0, y: 0 };
        let (mut master, mut slave): (c_int, c_int) = (-1, -1);
        // SAFETY: fds are checked; the child only calls async-signal-safe functions before execve.
        unsafe {
            if openpty(&mut master, &mut slave, std::ptr::null_mut(), std::ptr::null(), &size) != 0 {
                return Err(io::Error::last_os_error());
            }
            let pid = fork();
            if pid < 0 {
                let e = io::Error::last_os_error();
                close(master);
                close(slave);
                return Err(e);
            }
            if pid == 0 {
                setsid();
                ioctl(slave, TIOCSCTTY, 0);
                dup2(slave, 0);
                dup2(slave, 1);
                dup2(slave, 2);
                if slave > 2 {
                    close(slave);
                }
                close(master);
                chdir(dir.as_ptr());
                execve(path.as_ptr(), argv.as_ptr(), envp.as_ptr());
                _exit(127);
            }
            close(slave);
            let reader = libc_dup(master)?;
            Ok((Pty { master: File::from_raw_fd(master), pid }, Output(File::from_raw_fd(reader))))
        }
    }

    fn libc_dup(fd: RawFd) -> io::Result<RawFd> {
        unsafe extern "C" {
            fn dup(fd: c_int) -> c_int;
        }
        // SAFETY: duplicates a descriptor we own.
        let d = unsafe { dup(fd) };
        if d < 0 { Err(io::Error::last_os_error()) } else { Ok(d) }
    }

    impl Output {
        pub fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            match self.0.read(buf) {
                // The terminal's other side closed (EIO on macOS and Linux): the program ended.
                Err(e) if e.raw_os_error() == Some(5) => Ok(0),
                other => other,
            }
        }
    }

    impl Pty {
        pub fn write(&self, data: &[u8]) -> io::Result<()> {
            (&self.master).write_all(data)
        }

        pub fn resize(&self, cols: u16, rows: u16) -> io::Result<()> {
            use std::os::fd::AsRawFd;
            let size = Winsize { rows, cols, x: 0, y: 0 };
            // SAFETY: a live descriptor and a Winsize the call reads.
            let r = unsafe { ioctl(self.master.as_raw_fd(), TIOCSWINSZ, &size) };
            if r < 0 { Err(io::Error::last_os_error()) } else { Ok(()) }
        }

        pub fn pid(&self) -> u32 {
            self.pid as u32
        }

        pub fn wait(&self) {
            let mut status = 0;
            // SAFETY: waits for our own child.
            unsafe { waitpid(self.pid, &mut status, 0) };
        }

        pub fn close(&self) {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn windows_arguments_are_quoted_as_they_are_read_back() {
        assert_eq!(win_quote("plain"), "plain");
        assert_eq!(win_quote(""), "\"\"");
        assert_eq!(win_quote("two words"), "\"two words\"");
        assert_eq!(win_quote(r#"say "hi""#), r#""say \"hi\"""#);
        assert_eq!(win_quote(r"C:\dir with space\"), r#""C:\dir with space\\""#);
    }

    #[test]
    fn a_program_runs_in_a_pseudo_terminal() {
        let env: Vec<(String, String)> = std::env::vars().collect();
        let (program, args): (&str, Vec<String>) =
            if cfg!(windows) { ("cmd", vec!["/c".into(), "echo".into(), "hello-pty".into()]) } else { ("sh", vec!["-c".into(), "echo hello-pty; tty".into()]) };
        let cwd = std::env::temp_dir().display().to_string();
        let (pty, mut out) = Pty::spawn(program, &args, &cwd, &env, 80, 24).unwrap();
        assert!(pty.pid() > 0);
        let pty = std::sync::Arc::new(pty);
        let waiter = pty.clone();
        std::thread::spawn(move || waiter.wait());
        let mut seen = Vec::new();
        let _ = out.read_to_end(&mut seen);
        let text = String::from_utf8_lossy(&seen);
        assert!(text.contains("hello-pty"), "{text:?}");
    }
}
