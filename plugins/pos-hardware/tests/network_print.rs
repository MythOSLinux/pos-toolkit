//! The network transport against a local TCP listener standing in for a
//! :9100 ESC/POS printer — asserts the bytes that actually hit the wire.

use std::io::Read;
use std::net::TcpListener;

use tauri_plugin_pos_hardware::printing::{
    open_cash_drawer, print_job, Align, PrintOp, PrinterTarget, QrEcc,
};

/// Bind an ephemeral port and capture everything one connection sends.
fn capture_one_connection(listener: TcpListener) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        let mut bytes = Vec::new();
        stream.read_to_end(&mut bytes).expect("read");
        bytes
    })
}

#[test]
fn print_job_sends_init_text_and_cut() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let capture = capture_one_connection(listener);

    let target = PrinterTarget::Network { host: "127.0.0.1".into(), port };
    let ops = vec![
        PrintOp::Text {
            text: "HELLO KITCHEN".into(),
            bold: true,
            align: Some(Align::Center),
            size: Some((2, 2)),
        },
        PrintOp::Feed { lines: 2 },
        PrintOp::Cut,
    ];
    print_job(&target, &ops).expect("print_job");

    let bytes = capture.join().expect("capture thread");
    // ESC @ init
    assert_eq!(&bytes[..2], &[0x1B, 0x40]);
    let text_pos = bytes
        .windows(13)
        .position(|w| w == b"HELLO KITCHEN")
        .expect("text bytes on the wire");
    // ESC E 1 (bold) and GS ! 0x11 (double width+height) precede the text
    assert!(bytes[..text_pos].windows(3).any(|w| w == [0x1B, 0x45, 0x01]), "bold on");
    assert!(bytes[..text_pos].windows(3).any(|w| w == [0x1D, 0x21, 0x11]), "size 2x2");
    // GS V — paper cut — after the text
    assert!(bytes[text_pos..].windows(2).any(|w| w == [0x1D, 0x56]), "cut");
}

#[test]
fn print_job_sends_qrcode_store_and_print_commands() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let capture = capture_one_connection(listener);

    let target = PrinterTarget::Network { host: "127.0.0.1".into(), port };
    let payload = r#"{"v":1,"ref":"order_01K"}"#;
    print_job(
        &target,
        &[PrintOp::QrCode { data: payload.into(), size: Some(6), correction: Some(QrEcc::H) }],
    )
    .expect("print_job");

    let bytes = capture.join().expect("capture thread");
    // GS ( k — every QR setting and the store/print pair share this prefix
    assert!(bytes.windows(3).any(|w| w == [0x1D, 0x28, 0x6B]), "GS ( k");
    // fn 165 model: GS ( k 04 00 49 65 <model> 00, model 2 = 50
    assert!(
        bytes.windows(9).any(|w| w == [0x1D, 0x28, 0x6B, 0x04, 0x00, 0x31, 0x41, 0x32, 0x00]),
        "model 2 selected"
    );
    // fn 167 module size: … 49 67 06
    assert!(
        bytes.windows(8).any(|w| w == [0x1D, 0x28, 0x6B, 0x03, 0x00, 0x31, 0x43, 0x06]),
        "module size 6"
    );
    // fn 169 error correction: … 49 69 51  (51 = level H)
    assert!(
        bytes.windows(8).any(|w| w == [0x1D, 0x28, 0x6B, 0x03, 0x00, 0x31, 0x45, 0x33]),
        "correction level H"
    );
    // the payload itself reaches the wire, followed by fn 181 (print symbol)
    let data_pos = bytes
        .windows(payload.len())
        .position(|w| w == payload.as_bytes())
        .expect("qr payload on the wire");
    assert!(
        bytes[data_pos..].windows(8).any(|w| w == [0x1D, 0x28, 0x6B, 0x03, 0x00, 0x31, 0x51, 0x30]),
        "print symbol"
    );
}

#[test]
fn open_cash_drawer_sends_bare_kick_pulse() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let capture = capture_one_connection(listener);

    let target = PrinterTarget::Network { host: "127.0.0.1".into(), port };
    open_cash_drawer(&target).expect("open_cash_drawer");

    // Exactly the pin-2 pulse, no init (some printers feed paper on init).
    assert_eq!(capture.join().expect("capture thread"), vec![0x1B, 0x70, 0x00, 0x19, 0xFF]);
}
