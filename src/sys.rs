//! What a crate would otherwise give (ADR-0010): time and ISO 8601 dates, the operating system's random
//! numbers, the home folder, a program run with a time limit, a comparison in constant time, SHA-256, the
//! default browser.

use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

/// `2026-10-22T08:41:12.123Z`, as JavaScript's `toISOString`.
pub fn iso(ms: u64) -> String {
    let (days, rem) = ((ms / 86_400_000) as i64, ms % 86_400_000);
    let (y, m, d) = civil(days);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z", rem / 3_600_000, rem / 60_000 % 60, rem / 1000 % 60, rem % 1000)
}

/// Milliseconds since the epoch from an ISO 8601 date and time ending in `Z` or `+hh:mm`.
pub fn parse_iso(s: &str) -> Option<u64> {
    let b = s.as_bytes();
    let n = |at: usize, len: usize| -> Option<i64> { s.get(at..at + len)?.parse().ok() };
    if b.len() < 19 || b[4] != b'-' || b[7] != b'-' || !matches!(b[10], b'T' | b' ') || b[13] != b':' || b[16] != b':' {
        return None;
    }
    let (y, mo, d, h, mi, se) = (n(0, 4)?, n(5, 2)?, n(8, 2)?, n(11, 2)?, n(14, 2)?, n(17, 2)?);
    let mut i = 19;
    let mut ms = 0;
    if b.get(i) == Some(&b'.') {
        i += 1;
        let start = i;
        while b.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        let frac = &s[start..i];
        ms = format!("{:0<3}", &frac[..frac.len().min(3)]).parse().ok()?;
    }
    let offset = match b.get(i) {
        None | Some(b'Z') => 0,
        Some(&sign @ (b'+' | b'-')) => (if sign == b'-' { -1 } else { 1 }) * (n(i + 1, 2)? * 60 + n(i + 4, 2)?),
        _ => return None,
    };
    u64::try_from(days(y, mo, d) * 86_400_000 + ((h * 60 + mi - offset) * 60 + se) * 1000 + ms).ok()
}

// Days since 1970-01-01 and back, after Howard Hinnant's algorithms.
fn days(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * ((m + 9) % 12) + 2) / 5 + d - 1;
    era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468
}

fn civil(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// `n` random bytes from the operating system, in hex.
pub fn random_hex(n: usize) -> std::io::Result<String> {
    let mut buf = vec![0u8; n];
    os_random(&mut buf)?;
    Ok(buf.iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(windows)]
#[allow(unsafe_code)]
fn os_random(buf: &mut [u8]) -> std::io::Result<()> {
    #[link(name = "bcrypt")]
    unsafe extern "system" {
        fn BCryptGenRandom(algorithm: *mut std::ffi::c_void, buffer: *mut u8, len: u32, flags: u32) -> i32;
    }
    const BCRYPT_USE_SYSTEM_PREFERRED_RNG: u32 = 2;
    let len = u32::try_from(buf.len()).map_err(std::io::Error::other)?;
    // SAFETY: the pointer and the length describe `buf`; this flag takes no algorithm handle.
    let status = unsafe { BCryptGenRandom(std::ptr::null_mut(), buf.as_mut_ptr(), len, BCRYPT_USE_SYSTEM_PREFERRED_RNG) };
    if status == 0 { Ok(()) } else { Err(std::io::Error::other(format!("BCryptGenRandom failed: {status:#x}"))) }
}

#[cfg(not(windows))]
fn os_random(buf: &mut [u8]) -> std::io::Result<()> {
    std::fs::File::open("/dev/urandom")?.read_exact(buf)
}

pub fn home() -> PathBuf {
    std::env::home_dir().unwrap_or_default()
}

/// No console window for a child process on Windows.
pub fn hidden(cmd: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// The standard output of a program that succeeds within `limit`; None when it fails, cannot start or
/// runs too long (it is killed then).
pub fn output(cmd: &mut Command, limit: Duration) -> Option<String> {
    let mut child = hidden(cmd).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    let mut out = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut s = String::new();
        out.read_to_string(&mut s).map(|_| s)
    });
    let deadline = Instant::now() + limit;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return reader.join().ok()?.ok().filter(|_| status.success()),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

/// Equal strings, compared in a time that does not depend on where they differ.
pub fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Base64 (RFC 4648, with padding).
pub fn base64(data: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let n = (u32::from(chunk[0]) << 16) | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8) | u32::from(*chunk.get(2).unwrap_or(&0));
        for i in 0..4 {
            out.push(if i <= chunk.len() { ABC[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
        }
    }
    out
}

/// SHA-256 (FIPS 180-4) of `data`, in hex.
pub fn sha256(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut h: [u32; 8] = [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend((data.len() as u64 * 8).to_be_bytes());
    for block in msg.chunks(64) {
        let mut w = [0u32; 64];
        for (i, word) in block.chunks(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let t1 = hh.wrapping_add(e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25)).wrapping_add((e & f) ^ (!e & g)).wrapping_add(K[i]).wrapping_add(w[i]);
            let t2 = (a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22)).wrapping_add((a & b) ^ (a & c) ^ (b & c));
            (hh, g, f, e, d, c, b, a) = (g, f, e, d.wrapping_add(t1), c, b, a, t1.wrapping_add(t2));
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *x = x.wrapping_add(y);
        }
    }
    h.iter().map(|x| format!("{x:08x}")).collect()
}

/// The memory of each root process with all its descendants, in bytes: the working set on Windows, the
/// resident size elsewhere (ADR-0015). A root no longer running counts nothing.
pub fn tree_memory(roots: &[u32]) -> std::collections::HashMap<u32, u64> {
    let table = process_table();
    roots.iter().map(|&root| (root, descendants(&table, root).into_iter().filter_map(|pid| table.iter().find(|p| p.0 == pid).and_then(memory)).sum())).collect()
}

/// A process and every process below it, each once.
fn descendants(table: &[(u32, u32, Option<u64>)], root: u32) -> Vec<u32> {
    if !table.iter().any(|p| p.0 == root) {
        return Vec::new();
    }
    let mut out = vec![root];
    let mut i = 0;
    while i < out.len() {
        let parent = out[i];
        for &(pid, ppid, _) in table {
            if ppid == parent && pid != parent && !out.contains(&pid) {
                out.push(pid);
            }
        }
        i += 1;
    }
    out
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod processes {
    use std::ffi::c_void;

    #[repr(C)]
    struct Entry {
        size: u32,
        usage: u32,
        pid: u32,
        heap: usize,
        module: u32,
        threads: u32,
        ppid: u32,
        base: i32,
        flags: u32,
        exe: [u16; 260],
    }

    #[repr(C)]
    #[derive(Default)]
    struct Counters {
        cb: u32,
        faults: u32,
        peak_working_set: usize,
        working_set: usize,
        quota_peak_paged: usize,
        quota_paged: usize,
        quota_peak_non_paged: usize,
        quota_non_paged: usize,
        pagefile: usize,
        peak_pagefile: usize,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateToolhelp32Snapshot(flags: u32, pid: u32) -> *mut c_void;
        fn Process32FirstW(snapshot: *mut c_void, entry: *mut Entry) -> i32;
        fn Process32NextW(snapshot: *mut c_void, entry: *mut Entry) -> i32;
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
        fn K32GetProcessMemoryInfo(process: *mut c_void, counters: *mut Counters, size: u32) -> i32;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }

    const SNAP_PROCESS: u32 = 2;
    const QUERY_LIMITED: u32 = 0x1000;

    /// Every process and its parent, from one snapshot of the system.
    pub fn table() -> Vec<(u32, u32, Option<u64>)> {
        let mut out = Vec::new();
        // SAFETY: the snapshot handle is checked, closed once; `entry` is sized as the API asks.
        unsafe {
            let snap = CreateToolhelp32Snapshot(SNAP_PROCESS, 0);
            if snap.is_null() || snap as isize == -1 {
                return out;
            }
            let mut entry: Entry = std::mem::zeroed();
            entry.size = std::mem::size_of::<Entry>() as u32;
            let mut more = Process32FirstW(snap, &mut entry) != 0;
            while more {
                out.push((entry.pid, entry.ppid, None));
                more = Process32NextW(snap, &mut entry) != 0;
            }
            CloseHandle(snap);
        }
        out
    }

    /// The working set of a process this user may query.
    pub fn memory(pid: u32) -> Option<u64> {
        // SAFETY: the process handle is checked and closed; `c` is sized as the API asks.
        unsafe {
            let p = OpenProcess(QUERY_LIMITED, 0, pid);
            if p.is_null() {
                return None;
            }
            let mut c = Counters { cb: std::mem::size_of::<Counters>() as u32, ..Counters::default() };
            let ok = K32GetProcessMemoryInfo(p, &mut c, c.cb) != 0;
            CloseHandle(p);
            ok.then_some(c.working_set as u64)
        }
    }
}

#[cfg(not(windows))]
mod processes {
    /// Every process, its parent and its resident size, from `ps`.
    pub fn table() -> Vec<(u32, u32, Option<u64>)> {
        let out = super::output(std::process::Command::new("ps").args(["-axo", "pid=,ppid=,rss="]), std::time::Duration::from_secs(5)).unwrap_or_default();
        out.lines()
            .filter_map(|l| {
                let mut f = l.split_whitespace().map(str::parse::<u64>);
                Some((f.next()?.ok()? as u32, f.next()?.ok()? as u32, f.next()?.ok().map(|kb| kb * 1024)))
            })
            .collect()
    }

    pub fn memory(_pid: u32) -> Option<u64> {
        None
    }
}

fn process_table() -> Vec<(u32, u32, Option<u64>)> {
    processes::table()
}

fn memory(p: &(u32, u32, Option<u64>)) -> Option<u64> {
    p.2.or_else(|| processes::memory(p.0))
}

/// Opens an address in the user's default browser.
pub fn open_url(url: &str) -> std::io::Result<()> {
    let program = if cfg!(windows) {
        "explorer.exe"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    // ponytail: explorer.exe exits with 1 even when it opened the page: only a failure to start counts.
    hidden(&mut Command::new(program)).arg(url).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_dates_both_ways() {
        assert_eq!(iso(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(iso(1_792_658_472_123), "2026-10-22T08:41:12.123Z");
        assert_eq!(parse_iso("2026-10-22T08:41:12.123Z"), Some(1_792_658_472_123));
        assert_eq!(parse_iso("2026-10-22T10:41:12+02:00"), Some(1_792_658_472_000));
        assert_eq!(parse_iso("2024-02-29T00:00:00Z").map(iso).as_deref(), Some("2024-02-29T00:00:00.000Z"));
        assert_eq!(parse_iso("yesterday"), None);
    }

    #[test]
    fn random_and_compare() {
        let a = random_hex(24).unwrap();
        assert_eq!(a.len(), 48);
        assert_ne!(a, random_hex(24).unwrap());
        assert!(same(&a, &a.clone()));
        assert!(!same(&a, &a[1..]));
        assert!(!same("abc", "abd"));
    }

    #[test]
    fn a_process_tree_counts_each_process_once() {
        let table = [(1, 0, Some(10)), (2, 1, Some(20)), (3, 2, Some(30)), (4, 9, Some(40)), (5, 5, Some(50))];
        assert_eq!(descendants(&table, 1), [1, 2, 3]);
        assert_eq!(descendants(&table, 5), [5], "a process that is its own parent");
        assert!(descendants(&table, 7).is_empty(), "gone");
        let me = std::process::id();
        let mine = tree_memory(&[me])[&me];
        assert!(mine > 1_000_000, "this test's own process weighs something: {mine}");
    }

    #[test]
    fn base64_gives_the_rfc_4648_examples() {
        for (plain, coded) in [("", ""), ("f", "Zg=="), ("fo", "Zm8="), ("foo", "Zm9v"), ("foob", "Zm9vYg=="), ("fooba", "Zm9vYmE="), ("foobar", "Zm9vYmFy")] {
            assert_eq!(base64(plain.as_bytes()), coded);
        }
        assert_eq!(base64(&[0xff, 0xfe, 0xfd]), "//79");
    }

    #[test]
    fn sha256_gives_the_fips_180_4_examples() {
        assert_eq!(sha256(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(sha256(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(sha256(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"), "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1");
        assert_eq!(sha256(&[b'a'; 1_000_000]), "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0");
        // 55 and 56 bytes: the length no longer fits in the block, a second one is padded.
        assert_eq!(sha256(&[b'a'; 55]), "9f4390f8d30c2dd92ec9f095b65e2b9ae9b0a925a5258e241c9f1e910f734318");
        assert_eq!(sha256(&[b'a'; 56]), "b35439a4ac6f0948b6d6f9e3c6af0f5f590ce20f1bde7090ef7970686ec6738a");
    }
}
