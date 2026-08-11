//! Pins the JSON wire format hosts' frontends emit (via generated TS
//! bindings) for the printing types. Tag names and casing are load-bearing:
//! a serde attribute change here breaks every host silently.

use tauri_plugin_pos_hardware::printing::{Align, PrintOp, PrinterTarget, QrEcc};

#[test]
fn printer_target_wire_format() {
    let network: PrinterTarget =
        serde_json::from_str(r#"{"kind":"network","host":"192.168.1.50","port":9100}"#).expect("network");
    assert!(matches!(network, PrinterTarget::Network { ref host, port: 9100 } if host == "192.168.1.50"));

    // port is optional, defaults to 9100
    let default_port: PrinterTarget =
        serde_json::from_str(r#"{"kind":"network","host":"printer.local"}"#).expect("default port");
    assert!(matches!(default_port, PrinterTarget::Network { port: 9100, .. }));

    let usb: PrinterTarget =
        serde_json::from_str(r#"{"kind":"usb","vendorId":1208,"productId":514}"#).expect("usb");
    assert!(matches!(usb, PrinterTarget::Usb { vendor_id: 1208, product_id: 514 }));

    let system: PrinterTarget =
        serde_json::from_str(r#"{"kind":"system","name":"EPSON TM-T20III"}"#).expect("system");
    assert!(matches!(system, PrinterTarget::System { ref name } if name == "EPSON TM-T20III"));
}

#[test]
fn print_op_wire_format() {
    let ops: Vec<PrintOp> = serde_json::from_str(
        r#"[
            {"op":"text","text":"10 x Khinkali","bold":true,"align":"center","size":[2,2]},
            {"op":"text","text":"plain"},
            {"op":"image","path":"/tmp/logo.png","maxWidth":384},
            {"op":"qrCode","data":"{\"v\":1,\"ref\":\"order_01K\"}","size":6,"correction":"h"},
            {"op":"qrCode","data":"bare"},
            {"op":"feed","lines":3},
            {"op":"cut"},
            {"op":"drawerKick"},
            {"op":"raw","bytes":[27,64]}
        ]"#,
    )
    .expect("ops");

    assert!(matches!(
        &ops[0],
        PrintOp::Text { text, bold: true, align: Some(Align::Center), size: Some((2, 2)) } if text == "10 x Khinkali"
    ));
    assert!(matches!(&ops[1], PrintOp::Text { bold: false, align: None, size: None, .. }));
    assert!(matches!(&ops[2], PrintOp::Image { max_width: Some(384), .. }));
    assert!(matches!(
        &ops[3],
        PrintOp::QrCode { data, size: Some(6), correction: Some(QrEcc::H) } if data == r#"{"v":1,"ref":"order_01K"}"#
    ));
    // size and correction are optional — the print path supplies the defaults
    assert!(matches!(&ops[4], PrintOp::QrCode { size: None, correction: None, .. }));
    assert!(matches!(&ops[5], PrintOp::Feed { lines: 3 }));
    assert!(matches!(&ops[6], PrintOp::Cut));
    assert!(matches!(&ops[7], PrintOp::DrawerKick));
    assert!(matches!(&ops[8], PrintOp::Raw { ref bytes } if bytes == &[27, 64]));
}
