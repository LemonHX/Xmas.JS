//! zstd: the compressor's frames have to be frames.
//!
//! The compressor writes stored blocks, which is only worth anything if a decoder that is not
//! ours reads them. These tests hand the bytes to the `zstd` command line when it is on the
//! machine and compare the round trip either way.

use std::io::Write;
use std::process::{Command, Stdio};

use xmas_runtime::testkit::Harness;

/// Compress and decompress inside one realm, through the same host function eta calls.
fn round_trip(bytes: &[u8]) -> Vec<u8> {
    let mut h = Harness::with_modules();
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    let script = format!(
        r#"
        const raw = Uint8Array.from('{hex}'.match(/../g) ?? [], (pair) => parseInt(pair, 16));
        const packed = new Uint8Array(__zstd_run(true, raw, 0));
        const back = new Uint8Array(__zstd_run(false, packed, 0));
        globalThis.__packed = Array.from(packed);
        globalThis.__back = Array.from(back);
        "#
    );
    h.eval(&script).expect("round trip");
    let back: Vec<f64> = (0..h.expect_number("globalThis.__back.length") as usize)
        .map(|index| h.expect_number(&format!("globalThis.__back[{index}]")))
        .collect();
    back.into_iter().map(|value| value as u8).collect()
}

/// Decode a frame that already exists, without compressing it first.
fn unpacked(frame: &[u8]) -> Vec<u8> {
    let mut h = Harness::with_modules();
    let hex: String = frame.iter().map(|byte| format!("{byte:02x}")).collect();
    h.eval(&format!(
        r#"
        const raw = Uint8Array.from('{hex}'.match(/../g) ?? [], (pair) => parseInt(pair, 16));
        globalThis.__back = Array.from(new Uint8Array(__zstd_run(false, raw, 0)));
        "#
    ))
    .expect("decompress");
    let length = h.expect_number("globalThis.__back.length") as usize;
    (0..length)
        .map(|index| h.expect_number(&format!("globalThis.__back[{index}]")) as u8)
        .collect()
}

fn packed(bytes: &[u8]) -> Vec<u8> {
    let mut h = Harness::with_modules();
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    h.eval(&format!(
        r#"
        const raw = Uint8Array.from('{hex}'.match(/../g) ?? [], (pair) => parseInt(pair, 16));
        globalThis.__packed = Array.from(new Uint8Array(__zstd_run(true, raw, 0)));
        "#
    ))
    .expect("compress");
    let length = h.expect_number("globalThis.__packed.length") as usize;
    (0..length)
        .map(|index| h.expect_number(&format!("globalThis.__packed[{index}]")) as u8)
        .collect()
}

fn zstd_available() -> bool {
    Command::new("zstd")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

/// `zstd -d` on the frame, or None when there is no zstd to ask.
fn zstd_decodes(frame: &[u8]) -> Option<Result<Vec<u8>, String>> {
    if !zstd_available() {
        return None;
    }
    let mut child = Command::new("zstd")
        .args(["-d", "-c", "-q"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(frame)
        .expect("write frame");
    let output = child.wait_with_output().expect("zstd runs");
    Some(if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(String::from_utf8_lossy(&output.stderr).into_owned())
    })
}

#[test]
fn what_is_compressed_comes_back() {
    let input = b"the quick brown fox jumps over the lazy dog";
    assert_eq!(round_trip(input), input);
}

#[test]
fn an_empty_input_survives() {
    assert_eq!(round_trip(b""), b"");
}

#[test]
fn a_payload_that_does_not_fit_one_block_survives() {
    // Two stored blocks and a remainder: the header each one carries is the part that can be
    // wrong at a boundary.
    let input: Vec<u8> = (0..300_000u32).map(|value| (value % 251) as u8).collect();
    assert_eq!(round_trip(&input), input);
}

#[test]
fn a_payload_of_exactly_one_block_survives() {
    let input = vec![7u8; 128 * 1024];
    assert_eq!(round_trip(&input), input);
}

#[test]
fn the_real_zstd_reads_the_frame() {
    for input in [
        b"".to_vec(),
        b"short".to_vec(),
        (0..300_000u32).map(|value| (value % 251) as u8).collect(),
    ] {
        let frame = packed(&input);
        let Some(decoded) = zstd_decodes(&frame) else {
            return;
        };
        assert_eq!(decoded.expect("zstd decodes the frame"), input);
    }
}

#[test]
fn a_frame_the_real_zstd_wrote_is_read_here() {
    if !zstd_available() {
        return;
    }
    let input: Vec<u8> = (0..100_000u32).map(|value| (value % 251) as u8).collect();
    let mut child = Command::new("zstd")
        .args(["-c", "-q", "-19"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("zstd runs");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(&input)
        .expect("write input");
    let output = child.wait_with_output().expect("zstd runs");
    assert!(output.status.success());
    // A frame that is compressed, not stored: the decoder has to actually decode it.
    assert!(output.stdout.len() < input.len() / 2, "{} bytes", output.stdout.len());
    assert_eq!(unpacked(&output.stdout), input);
}

#[test]
fn the_limit_is_honoured_when_decompressing() {
    let mut h = Harness::with_modules();
    let frame = packed(&vec![9u8; 1000]);
    let list: Vec<String> = frame.iter().map(|byte| byte.to_string()).collect();
    h.eval(&format!(
        "globalThis.__frame = Uint8Array.from([{}]);",
        list.join(",")
    ))
    .expect("frame in the realm");
    let outcome = h.eval("__zstd_run(false, globalThis.__frame, 10)");
    let message = outcome.expect_err("a limit of 10 refuses 1000 bytes");
    assert!(message.contains("RangeError"), "{message}");
}

#[test]
fn zlib_exposes_zstd_the_way_node_does() {
    let mut h = Harness::with_modules();
    let input: Vec<u8> = (0..1000u32).map(|value| (value % 251) as u8).collect();
    h.eval(&format!(
        r#"
        const zlib = globalThis.__zlib_module;
        const raw = Uint8Array.from([{}]);
        const packed = zlib.zstdCompressSync(raw);
        globalThis.__back = Array.from(zlib.zstdDecompressSync(packed));
        globalThis.__fromBun = Array.from(zlib.zstdDecompressSync(zlib.zstdCompressSync(raw)));
        "#,
        input.iter().map(|byte| byte.to_string()).collect::<Vec<_>>().join(",")
    ))
    .expect("zstd through zlib");
    let length = h.expect_number("globalThis.__back.length") as usize;
    let back: Vec<u8> = (0..length)
        .map(|index| h.expect_number(&format!("globalThis.__back[{index}]")) as u8)
        .collect();
    assert_eq!(back, input);
    let length = h.expect_number("globalThis.__fromBun.length") as usize;
    assert_eq!(length, input.len());
}

#[test]
fn zstd_reports_a_bad_payload_as_a_data_error() {
    let mut h = Harness::with_modules();
    let outcome = h.eval(
        r#"
        try {
          globalThis.__zlib_module.zstdDecompressSync(Uint8Array.from([1, 2, 3, 4, 5]));
          'no error';
        } catch (thrown) {
          globalThis.__code = thrown.code ?? thrown.name;
          String(globalThis.__code);
        }
        "#,
    );
    outcome.expect("a bad payload is reported");
    assert_eq!(h.expect_text("globalThis.__code"), "Z_DATA_ERROR");
}
