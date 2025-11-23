use std::path::PathBuf;

use zmk_layout_rs::flash::{
    FlashDiscovery, FlashOutcome, FlashSide, render_device, render_flash_outcome,
    render_flash_warning,
};

#[test]
fn device_listing_matches_snapshot() {
    let devices = vec![
        FlashDiscovery {
            name: "GLV80_BOOT".into(),
            dev_path: Some(PathBuf::from("/dev/sda1")),
            mountpoints: vec![PathBuf::from("/media/GLV80")],
            serial: Some("SER123".into()),
            vendor: Some("MoErgo".into()),
            model: Some("Glove80 Boot".into()),
            fs_type: Some("vfat".into()),
            removable: Some(true),
            vendor_id: Some("2341".into()),
            product_id: Some("1234".into()),
        },
        FlashDiscovery {
            name: "OTHER_DEV".into(),
            dev_path: Some(PathBuf::from("/dev/sdb1")),
            mountpoints: Vec::new(),
            serial: None,
            vendor: None,
            model: None,
            fs_type: None,
            removable: Some(false),
            vendor_id: None,
            product_id: None,
        },
    ];
    let rendered = devices
        .iter()
        .map(render_device)
        .collect::<Vec<_>>()
        .join("\n");
    let expected = include_str!("fixtures/flash_devices_snapshot.txt");
    assert_eq!(rendered + "\n", expected);
}

#[test]
fn flash_progress_matches_snapshot() {
    let outcome = FlashOutcome {
        side: FlashSide::Left,
        artifact: PathBuf::from("left.uf2"),
        mountpoint: PathBuf::from("/media/GLV80"),
        bytes_written: 4096,
        warnings: vec![
            "flashed device serial SER123".into(),
            "could not read board-id from /media/GLV80 to verify side".into(),
        ],
    };
    let summary = render_flash_outcome(&outcome);
    let warnings = outcome
        .warnings
        .iter()
        .map(|w| render_flash_warning(w))
        .collect::<Vec<_>>()
        .join("\n");
    let output = format!("{summary}\n{warnings}\n");
    let expected = include_str!("fixtures/flash_progress_snapshot.txt");
    assert_eq!(output, expected);
}
