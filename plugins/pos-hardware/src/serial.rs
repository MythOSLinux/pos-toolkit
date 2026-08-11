//! Serial (Virtual COM) peripherals — today, barcode scanners.
//!
//! ## Why a POS wants this and not a keyboard wedge
//!
//! A wedge scanner does not send data. It synthesises **keystrokes**, which the
//! operating system then resolves through whatever keyboard layout happens to
//! be active, and the host application reads the result out of its focused
//! input. Three problems follow, and all three are structural rather than
//! bugs to be fixed:
//!
//! 1. **The layout rewrites the payload.** A scanner whose HID country code
//!    disagrees with the host delivers the right letters and the wrong
//!    punctuation — `:` arrives as `Ö`, `/` as `-`, and a dead key destroys the
//!    character after it outright. Observed on a Tera HW0009 against a Swedish
//!    host, 2026-08-11.
//! 2. **Whatever has focus receives the scan.** A scan can land in a table-name
//!    box, a PIN field, or a search filter. Applications work around this with
//!    global key capture and timing heuristics ("a person cannot type this
//!    fast"), which is guesswork about intent.
//! 3. **It is slow.** Keystrokes are injected with an inter-character delay, so
//!    a 350-character 2D payload takes seconds. The same bytes at 9600 baud
//!    take ~0.36s, and at 115200 about 30ms.
//!
//! A serial connection has none of them. It carries **bytes**, addressed to
//! whoever opened the port, at line speed. Every scanner worth buying can be
//! switched to it with a configuration barcode, usually labelled "USB Virtual
//! COM", "USB-COM" or "CDC".
//!
//! ## What this module does not decide
//!
//! Nothing here knows what a barcode *means*. It frames bytes into messages and
//! hands them up — the host owns encoding policy, recognition and everything
//! after. Both `text` (UTF-8, lossy) and the raw `bytes` ride every scan
//! deliberately: a scanner set to the wrong output encoding is a real failure
//! mode, and a host that only ever sees a lossy string cannot diagnose it.
//!
//! ## Platform notes
//!
//! - **Linux:** the port is typically `/dev/ttyACM0` (CDC) or `/dev/ttyUSB0`.
//!   The user must be in the serial group — `dialout` on Debian/Ubuntu,
//!   **`uucp` on Arch/Manjaro** — or opening it fails with a permission error.
//!   This is the single commonest reason a correctly-configured scanner appears
//!   dead, so [`open_error_hint`] turns that error into the sentence that fixes
//!   it rather than leaving "Permission denied" on screen.
//! - **Windows:** `COM3` and friends. CDC-class devices need no driver on
//!   Windows 10+; some vendors still ship one.
//! - **macOS:** `/dev/cu.usbmodem*`. Prefer `cu.` over `tty.` — the latter
//!   blocks on carrier detect.

use std::collections::HashMap;
use std::io::ErrorKind;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Runtime};

#[cfg(feature = "specta")]
use specta::Type;

/// Longest message this will buffer before flushing it as `overflow`.
///
/// A scanner that never sends a terminator would otherwise accumulate until
/// memory ran out. 4 KiB is far above any real 2D payload.
pub const MAX_MESSAGE_LEN: usize = 4096;

/// The event a framed scan is emitted on.
pub const SCAN_EVENT: &str = "pos-hardware://serial-scan";

/// The event a reader emits when it stops on an error rather than on request.
pub const ERROR_EVENT: &str = "pos-hardware://serial-error";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(Type))]
#[serde(rename_all = "camelCase")]
pub struct SerialPortInfo {
    /// What to pass back to [`open_scanner`]. `/dev/ttyACM0`, `COM3`, …
    pub path: String,
    /// `usb`, `bluetooth`, `pci` or `unknown` — how the port is attached.
    pub kind: String,
    pub vid: Option<u16>,
    pub pid: Option<u16>,
    pub manufacturer: Option<String>,
    pub product: Option<String>,
    /// The only field that reliably distinguishes two identical scanners.
    pub serial_number: Option<String>,
    /// Worth offering to a human: a USB or Bluetooth port, never a legacy
    /// `ttyS*`. A picker should show these and put the rest behind "show all".
    pub likely: bool,
    /// Linux `/dev/serial/by-id/…`, where one exists.
    ///
    /// **Prefer this over `path` when storing a choice.** `ttyUSB0` is assigned
    /// in enumeration order, so unplugging a scanner and plugging it back — or
    /// a device that re-enumerates on its own — can move it to `ttyUSB1` and
    /// silently point a saved setting at nothing. The by-id symlink is derived
    /// from the device's own identity and survives that.
    pub stable_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(Type))]
#[serde(rename_all = "camelCase")]
pub struct SerialScan {
    /// Which port it came from — a venue may have a scanner and a scale.
    pub port: String,
    /// UTF-8, lossy. What the host will normally act on.
    pub text: String,
    /// Exactly what arrived. Kept so a wrong output encoding is diagnosable
    /// rather than merely visible as mojibake.
    pub bytes: Vec<u8>,
    /// `cr`, `lf`, `crlf`, `idle` or `overflow`.
    pub terminated_by: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(Type))]
#[serde(rename_all = "camelCase")]
pub struct SerialError {
    pub port: String,
    pub message: String,
}

/// Open readers, keyed by port path. Each holds the flag that stops its thread.
#[derive(Default)]
pub struct SerialState(Mutex<HashMap<String, Arc<AtomicBool>>>);

/// Ports the system knows about.
///
/// Every serial port is returned, not just plausible scanners: a POS may also
/// be talking to a scale, a customer display or a payment terminal, and
/// deciding which is which from a VID/PID table would be wrong the first time a
/// venue bought a model the table had never heard of. The host presents the
/// list and a human picks.
///
/// **But the raw list is unusable on Linux**, which is why [`SerialPortInfo`]
/// carries `likely` and `stable_path`. A stock desktop reports 32 legacy
/// `/dev/ttyS*` ports that have never had anything attached to them (measured
/// on Manjaro, 2026-08-11 — with *and* without libudev, so the feature flag is
/// not the fix). A picker that lists those and the scanner together asks the
/// operator a question they cannot answer.
pub fn list_serial_ports() -> Result<Vec<SerialPortInfo>, String> {
    let ports = serialport::available_ports().map_err(|e| format!("could not list serial ports: {e}"))?;
    Ok(ports
        .into_iter()
        .map(|p| {
            let (kind, vid, pid, manufacturer, product, serial_number) = match p.port_type {
                serialport::SerialPortType::UsbPort(info) => (
                    "usb",
                    Some(info.vid),
                    Some(info.pid),
                    info.manufacturer,
                    info.product,
                    info.serial_number,
                ),
                serialport::SerialPortType::BluetoothPort => ("bluetooth", None, None, None, None, None),
                serialport::SerialPortType::PciPort => ("pci", None, None, None, None, None),
                serialport::SerialPortType::Unknown => ("unknown", None, None, None, None, None),
            };
            let likely = kind == "usb" || kind == "bluetooth" || is_usb_serial_name(&p.port_name);
            let stable_path = stable_path_for(&p.port_name);
            SerialPortInfo {
                path: p.port_name,
                kind: kind.to_string(),
                vid,
                pid,
                manufacturer,
                product,
                serial_number,
                likely,
                stable_path,
            }
        })
        .collect())
}

/// A name only a USB-attached serial device gets.
///
/// The fallback for when the port type says `Unknown`, which on Linux it often
/// does — the sysfs enumeration used without libudev classifies almost nothing,
/// and even with libudev a plain `ttyS*` comes back unknown rather than PCI.
/// The naming convention is stable across every Linux distribution: `ttyUSB*`
/// for a vendor bridge (CH340, FTDI, PL2303), `ttyACM*` for a class-compliant
/// CDC device, `cu.usb*` on macOS.
fn is_usb_serial_name(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.starts_with("ttyUSB")
        || name.starts_with("ttyACM")
        || name.starts_with("cu.usb")
        || name.starts_with("tty.usb")
}

/// The `/dev/serial/by-id/…` symlink pointing at this port, if the OS made one.
///
/// Linux only, and only for USB devices — which is exactly the case where the
/// unstable name matters, because `ttyUSB0` is handed out in enumeration order.
/// Read by resolving each symlink rather than by constructing a name: the
/// encoding of the id (vendor, product, serial, interface) has changed between
/// udev versions and is not ours to reproduce.
fn stable_path_for(path: &str) -> Option<String> {
    let dir = std::path::Path::new("/dev/serial/by-id");
    let target = std::fs::canonicalize(path).ok()?;
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let link = entry.path();
        if std::fs::canonicalize(&link).ok().as_deref() == Some(target.as_path()) {
            return Some(link.to_string_lossy().into_owned());
        }
    }
    None
}

/// Turn an open failure into something a person can act on.
///
/// "Permission denied" is true and useless: it is almost always group
/// membership, and the group differs by distribution. Naming both candidates is
/// better than naming the wrong one confidently.
pub fn open_error_hint(path: &str, err: &serialport::Error) -> String {
    let base = format!("could not open {path}: {err}");
    match err.kind() {
        serialport::ErrorKind::NoDevice => {
            format!("{base}. The port is not there — check the scanner is plugged in and switched to Virtual COM mode rather than keyboard mode.")
        }
        serialport::ErrorKind::Io(ErrorKind::PermissionDenied) => format!(
            "{base}. On Linux this is normally group membership: add the user to `uucp` (Arch/Manjaro) or `dialout` (Debian/Ubuntu) and log out and back in."
        ),
        _ => base,
    }
}

/// Where a message ended.
fn frame(buffer: &mut Vec<u8>, byte: u8, last_was_cr: bool) -> Option<&'static str> {
    match byte {
        b'\r' => Some("cr"),
        // A CRLF pair must not produce an empty second message, so an LF
        // arriving straight after a CR is part of the same terminator.
        b'\n' if last_was_cr => None,
        b'\n' => Some("lf"),
        _ => {
            buffer.push(byte);
            None
        }
    }
}

/// Start reading `path`, emitting one [`SerialScan`] per framed message.
///
/// Idempotent per port: opening a port that is already open is a no-op rather
/// than a second reader racing the first for bytes.
///
/// `idle_ms` frames a scanner that sends no terminator at all. It is a fallback,
/// not the normal path — a scanner in COM mode almost always sends CR — and it
/// costs one `idle_ms` of latency on every scan when it is the only framing, so
/// a host should prefer configuring the terminator on the device.
pub fn open_scanner<R: Runtime>(
    app: AppHandle<R>,
    state: &SerialState,
    path: String,
    baud: u32,
    idle_ms: u64,
) -> Result<(), String> {
    {
        let open = state.0.lock().map_err(|_| "serial state poisoned".to_string())?;
        if open.contains_key(&path) {
            return Ok(());
        }
    }

    // Opened once here so a mistake the operator can fix — wrong port, no
    // permission — is reported synchronously instead of disappearing into a
    // thread. Everything after this is the supervisor's problem.
    let port = serialport::new(&path, baud)
        // Short enough that the stop flag is honoured promptly and the idle
        // framer has a tick to run on; long enough not to spin a core.
        .timeout(Duration::from_millis(25))
        .open()
        .map_err(|e| open_error_hint(&path, &e))?;

    let stop = Arc::new(AtomicBool::new(false));
    {
        let mut open = state.0.lock().map_err(|_| "serial state poisoned".to_string())?;
        open.insert(path.clone(), Arc::clone(&stop));
    }

    let thread_path = path.clone();
    std::thread::Builder::new()
        .name(format!("pos-serial:{path}"))
        .spawn(move || {
            supervise(app, port, thread_path, baud, idle_ms, stop);
        })
        .map_err(|e| format!("could not start reader for {path}: {e}"))?;

    Ok(())
}

/// How long to wait between reconnection attempts, and how long before the
/// operator is told. Re-enumeration takes a second or two; a scanner that is
/// simply unplugged for the night should not fill a log.
const RECONNECT_DELAY: Duration = Duration::from_millis(750);
const RECONNECT_QUIET_ATTEMPTS: u32 = 8;

/// Keep a reader on the port for as long as the host wants one.
///
/// ⚠ **A serial device disappearing is normal, not exceptional.** Some scanners
/// re-enumerate between scans; a USB hub browns out; someone knocks the cable.
/// The first version of this returned on the first read error, which left the
/// port dead until the operator changed a setting — indistinguishable, from the
/// floor, from a scanner nobody had scanned with.
///
/// So an error reopens rather than exits, and the operator is told only once
/// the failure has persisted past `RECONNECT_QUIET_ATTEMPTS` — otherwise an
/// ordinary re-enumeration would raise an alarm every time somebody scanned.
fn supervise<R: Runtime>(
    app: AppHandle<R>,
    first: Box<dyn serialport::SerialPort>,
    path: String,
    baud: u32,
    idle_ms: u64,
    stop: Arc<AtomicBool>,
) {
    let mut port = Some(first);
    let mut failures: u32 = 0;

    loop {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        if let Some(open) = port.take() {
            read_loop(&app, open, &path, idle_ms, &stop);
            // read_loop only returns on stop or on a dead port.
            if stop.load(Ordering::Relaxed) {
                return;
            }
        }

        std::thread::sleep(RECONNECT_DELAY);
        if stop.load(Ordering::Relaxed) {
            return;
        }
        match serialport::new(&path, baud)
            .timeout(Duration::from_millis(25))
            .open()
        {
            Ok(reopened) => {
                if failures >= RECONNECT_QUIET_ATTEMPTS {
                    // Only worth saying because the operator was told it broke.
                    let _ = app.emit(
                        ERROR_EVENT,
                        SerialError { port: path.clone(), message: String::new() },
                    );
                }
                failures = 0;
                port = Some(reopened);
            }
            Err(e) => {
                failures += 1;
                if failures == RECONNECT_QUIET_ATTEMPTS {
                    let _ = app.emit(
                        ERROR_EVENT,
                        SerialError { port: path.clone(), message: open_error_hint(&path, &e) },
                    );
                }
            }
        }
    }
}

/// Read one open port until it stops or dies. Reconnection is [`supervise`]'s.
fn read_loop<R: Runtime>(
    app: &AppHandle<R>,
    mut port: Box<dyn serialport::SerialPort>,
    path: &str,
    idle_ms: u64,
    stop: &Arc<AtomicBool>,
) {
    let mut buffer: Vec<u8> = Vec::with_capacity(256);
    let mut chunk = [0u8; 256];
    let mut last_was_cr = false;
    let mut last_byte_at = Instant::now();

    let emit = |app: &AppHandle<R>, buffer: &mut Vec<u8>, terminated_by: &str| {
        if buffer.is_empty() {
            return;
        }
        let bytes = std::mem::take(buffer);
        let scan = SerialScan {
            port: path.to_string(),
            text: String::from_utf8_lossy(&bytes).into_owned(),
            bytes,
            terminated_by: terminated_by.to_string(),
        };
        if let Err(e) = app.emit(SCAN_EVENT, scan) {
            log::warn!("serial scan emit failed on {path}: {e}");
        }
    };

    loop {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        match port.read(&mut chunk) {
            Ok(0) => {}
            Ok(n) => {
                last_byte_at = Instant::now();
                for &byte in &chunk[..n] {
                    let was_cr = last_was_cr;
                    last_was_cr = byte == b'\r';
                    if let Some(terminator) = frame(&mut buffer, byte, was_cr) {
                        emit(app, &mut buffer, terminator);
                    } else if buffer.len() >= MAX_MESSAGE_LEN {
                        emit(app, &mut buffer, "overflow");
                    }
                }
            }
            Err(e) if e.kind() == ErrorKind::TimedOut => {
                // The only framing a terminator-less scanner gets.
                if idle_ms > 0
                    && !buffer.is_empty()
                    && last_byte_at.elapsed() >= Duration::from_millis(idle_ms)
                {
                    emit(app, &mut buffer, "idle");
                }
            }
            Err(_) => {
                // The device went away. Hand back to the supervisor, which
                // reopens — a scanner that re-enumerates between scans is a
                // real device behaviour, not a fault to report on sight.
                return;
            }
        }
    }
}

/// Stop reading `path`. Unknown ports are not an error — closing twice is fine.
pub fn close_scanner(state: &SerialState, path: &str) -> Result<(), String> {
    let mut open = state.0.lock().map_err(|_| "serial state poisoned".to_string())?;
    if let Some(stop) = open.remove(path) {
        stop.store(true, Ordering::Relaxed);
    }
    Ok(())
}

/// Which ports currently have a reader attached.
pub fn open_scanners(state: &SerialState) -> Result<Vec<String>, String> {
    let open = state.0.lock().map_err(|_| "serial state poisoned".to_string())?;
    Ok(open.keys().cloned().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Framing is the whole protocol, so it is tested without a port.
    fn frames(input: &[u8]) -> Vec<(String, String)> {
        let mut buffer = Vec::new();
        let mut out = Vec::new();
        let mut last_was_cr = false;
        for &byte in input {
            let was_cr = last_was_cr;
            last_was_cr = byte == b'\r';
            if let Some(t) = frame(&mut buffer, byte, was_cr) {
                if !buffer.is_empty() {
                    out.push((String::from_utf8_lossy(&buffer).into_owned(), t.to_string()));
                    buffer.clear();
                }
            }
        }
        out
    }

    #[test]
    fn frames_on_cr() {
        assert_eq!(frames(b"hello\r"), vec![("hello".into(), "cr".into())]);
    }

    #[test]
    fn frames_on_lf() {
        assert_eq!(frames(b"hello\n"), vec![("hello".into(), "lf".into())]);
    }

    #[test]
    fn treats_crlf_as_one_terminator() {
        // The regression this guards: an LF after a CR flushing a second,
        // empty message — which downstream would read as a scan of nothing.
        assert_eq!(frames(b"hello\r\n"), vec![("hello".into(), "cr".into())]);
        assert_eq!(frames(b"a\r\nb\r\n"), vec![("a".into(), "cr".into()), ("b".into(), "cr".into())]);
    }

    #[test]
    fn keeps_consecutive_scans_apart() {
        assert_eq!(
            frames(b"first\rsecond\r"),
            vec![("first".into(), "cr".into()), ("second".into(), "cr".into())]
        );
    }

    #[test]
    fn carries_utf8_through_untouched() {
        // The point of the whole transport: bytes, not keystrokes. A Georgian
        // name cannot be mangled by a keyboard layout that is not involved.
        let ka = "წინანდალი";
        let mut input = ka.as_bytes().to_vec();
        input.push(b'\r');
        assert_eq!(frames(&input), vec![(ka.to_string(), "cr".into())]);
    }
}
