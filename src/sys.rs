//! What a crate would otherwise give (ADR-0010): time and ISO 8601 dates, the operating system's random
//! numbers, the home folder, a program run with a time limit, a comparison in constant time.

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
}
