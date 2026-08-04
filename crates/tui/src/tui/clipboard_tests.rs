// Tests relocated from src/tui/clipboard.rs

use super::*;
use std::borrow::Cow;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

fn solid_rgba(width: u16, height: u16, rgba: [u8; 4]) -> ImageData<'static> {
    let mut bytes = Vec::with_capacity((width as usize) * (height as usize) * 4);
    for _ in 0..(width as usize * height as usize) {
        bytes.extend_from_slice(&rgba);
    }
    ImageData {
        width: width as usize,
        height: height as usize,
        bytes: Cow::Owned(bytes),
    }
}

#[test]
fn save_image_as_png_writes_valid_png() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let img = solid_rgba(8, 4, [255, 0, 0, 255]);
    let pasted = save_image_as_png_in(dir.path(), &img).expect("encode png");

    assert_eq!(pasted.width, 8);
    assert_eq!(pasted.height, 4);
    assert!(pasted.byte_len > 0);
    assert_eq!(
        pasted.path.extension().and_then(|s| s.to_str()),
        Some("png")
    );

    // The first eight bytes of any PNG file are the magic signature; if
    // we ever regress to PPM or another format this will catch it.
    let header = std::fs::read(&pasted.path).expect("read file");
    assert_eq!(&header[..8], b"\x89PNG\r\n\x1a\n");
}

#[test]
fn clipboard_images_dir_uses_mimofan_home_directory() {
    let home = tempfile::tempdir().expect("create temp dir");
    let workspace = tempfile::tempdir().expect("create temp dir");

    assert_eq!(
        clipboard_images_dir_for_home(workspace.path(), Some(home.path())),
        home.path().join(".mimofan").join("clipboard-images")
    );
}

#[test]
fn clipboard_images_dir_falls_back_to_workspace_without_home() {
    let workspace = tempfile::tempdir().expect("create temp dir");

    assert_eq!(
        clipboard_images_dir_for_home(workspace.path(), None),
        workspace.path().join("clipboard-images")
    );
}

#[test]
fn pasted_image_labels_format_correctly() {
    let p = PastedImage {
        path: PathBuf::from("/tmp/x.png"),
        width: 1024,
        height: 768,
        byte_len: 235 * 1024,
    };
    assert_eq!(p.short_label(), "1024x768 PNG");
    assert_eq!(p.size_label(), "235KB");
}

#[test]
fn osc52_sequence_encodes_text_clipboard_write() {
    let sequence = osc52_sequence("hello", false).expect("sequence");
    assert_eq!(sequence, "\x1b]52;c;aGVsbG8=\x07");
}

#[test]
fn osc52_sequence_wraps_for_tmux_passthrough() {
    let sequence = osc52_sequence("copy", true).expect("sequence");
    assert_eq!(sequence, "\x1bPtmux;\x1b\x1b]52;c;Y29weQ==\x07\x1b\\");
}

#[test]
fn osc52_sequence_rejects_oversized_selection() {
    let text = "x".repeat(OSC52_MAX_BYTES + 1);
    let err = osc52_sequence(&text, false).expect_err("oversized should fail");
    assert!(
        err.to_string().contains("too large"),
        "unexpected error: {err}"
    );
}

#[cfg(unix)]
#[test]
fn wl_paste_helper_reads_text_from_stdout() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let script = dir.path().join("wl-paste");
    std::fs::write(
        &script,
        r#"#!/bin/sh
seen_no_newline=0
seen_text_plain=0
while [ "$#" -gt 0 ]; do
  case "$1" in
    --no-newline) seen_no_newline=1 ;;
    --type)
      shift
      [ "${1:-}" = "text/plain" ] && seen_text_plain=1
      ;;
  esac
  shift
done
[ "$seen_text_plain" -eq 1 ] || exit 40
if [ "$seen_no_newline" -eq 1 ]; then
  printf 'from-wayland'
else
  printf 'from-wayland\n'
fi
"#,
    )
    .expect("unexpected None/Err in test");
    let mut perms = std::fs::metadata(&script)
        .expect("read file metadata")
        .permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&script, perms).expect("set file permissions");

    let text = read_text_with_wlpaste_using_argv(script.to_str().expect("convert OsStr to str"))
        .expect("read text through wl-paste helper");

    assert_eq!(text, "from-wayland");
}
