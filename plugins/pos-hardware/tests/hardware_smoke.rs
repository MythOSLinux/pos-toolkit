//! Real-hardware smoke tests — `#[ignore]`d so CI never needs a printer.
//! Point PRINTER_HOST (and optionally PRINTER_PORT) at a network ESC/POS
//! printer and run explicitly:
//!
//! ```sh
//! PRINTER_HOST=192.168.1.153 cargo test -p tauri-plugin-pos-hardware \
//!     --test hardware_smoke -- --ignored --nocapture
//! ```
//!
//! `print_test_page` uses one short strip of paper and exercises init, bold,
//! size multipliers, ASCII punctuation, and a UTF-8 Georgian line (whether it
//! renders is a firmware fact worth knowing per venue). `kick_drawer` sends
//! the bare pin-2 pulse — the drawer should pop, no paper should move.

use tauri_plugin_pos_hardware::printing::{open_cash_drawer, print_job, query, Align, PrintOp, PrinterTarget};

fn target() -> PrinterTarget {
    let host = std::env::var("PRINTER_HOST").expect("set PRINTER_HOST to run hardware smoke tests");
    let port = std::env::var("PRINTER_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(9100);
    PrinterTarget::Network { host, port }
}

fn text(text: &str) -> PrintOp {
    PrintOp::Text { text: text.into(), bold: false, align: None, size: None }
}

#[test]
#[ignore = "needs a real printer (PRINTER_HOST)"]
fn print_test_page() {
    let ops = vec![
        PrintOp::Text {
            text: "POS-TOOLKIT TEST".into(),
            bold: true,
            align: Some(Align::Center),
            size: None,
        },
        PrintOp::Text {
            text: "BIG 2x2".into(),
            bold: true,
            align: Some(Align::Center),
            size: Some((2, 2)),
        },
        text("------------------------------------------------"),
        text("ascii + punctuation: ()*%#@!?0123456789"),
        PrintOp::Text { text: "tall 1x2 line".into(), bold: true, align: None, size: Some((1, 2)) },
        text("utf8 georgian: \u{10ee}\u{10d8}\u{10dc}\u{10d9}\u{10d0}\u{10da}\u{10d8}"),
        text("(garbled/blank above = no UTF-8 firmware)"),
        text("------------------------------------------------"),
        PrintOp::Feed { lines: 3 },
        PrintOp::Cut,
    ];
    print_job(&target(), &ops).expect("print_job against the real printer");
}

#[test]
#[ignore = "needs a real printer with a drawer (PRINTER_HOST)"]
fn kick_drawer() {
    open_cash_drawer(&target()).expect("drawer kick against the real printer");
}

/// What the printer says about itself — no paper moves.
///
/// `GS I n` transmits the printer id: 65 firmware, 66 manufacturer, 67 model,
/// 68 serial, 69 font language. The reply is framed `0x5F <data> 0x00`.
/// `DLE EOT n` is real-time status, answered even mid-print.
///
/// Nothing is asserted about the *values* — they are per-machine facts, and
/// the point of the test is to read them out loud. What IS asserted: a head
/// that answers at all answers with the documented framing, and a query it
/// does not implement comes back empty rather than as an error.
#[test]
#[ignore = "needs a printer; set PRINTER_HOST"]
fn query_identity_and_status() {
    let t = target();
    for (n, what) in [(65u8, "firmware"), (66, "manufacturer"), (67, "model"), (68, "serial"), (69, "font language")] {
        let reply = query(&t, &[0x1d, b'I', n], 1500).expect("query");
        let text = String::from_utf8_lossy(&reply)
            .trim_matches(|c: char| c == '_' || c == '\0')
            .to_string();
        println!("GS I {n:<3} {what:<14} {text:?}  (raw {reply:02x?})");
        if !reply.is_empty() {
            assert_eq!(reply[0], 0x5f, "documented header byte");
            assert_eq!(*reply.last().expect("non-empty"), 0x00, "documented terminator");
        }
    }
    for (n, what) in [(1u8, "printer"), (2, "offline cause"), (3, "error"), (4, "paper roll")] {
        let reply = query(&t, &[0x10, 0x04, n], 1500).expect("query");
        println!("DLE EOT {n} {what:<14} {reply:02x?}");
        assert_eq!(reply.len(), 1, "real-time status is a single byte");
    }
    // A query this firmware does not implement: silence, not a failure. The
    // Epson memory-switch query is the example — clones ignore it entirely.
    let unimplemented = query(&t, &[0x1d, b'(', b'E', 0x02, 0x00, 0x04, b'1'], 800).expect("no error for silence");
    println!("GS ( E fn=4 (memory switch) {} bytes", unimplemented.len());
}
