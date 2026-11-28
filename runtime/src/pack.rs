//! Self-extracting binary support.
//!
//! `xmas compile` appends a gzip-compressed tar payload to the xmas binary and writes a
//! 16-byte trailer at the very end:
//!
//!   [ payload_len: u64 LE ][ b"XMASPKG\0" ]
//!
//! On startup, `try_run_embedded` reads the last 16 bytes of the running binary, checks the
//! magic, seeks back to the payload, extracts it to `~/.xmas/apps/<sha256[:16]>/` (skipping
//! extraction when the dir already exists), and runs `main.js` from there.
//!
//! The pack/unpack logic lives here, not behind the `compile` feature, so a vanilla xmas
//! binary can run a compiled app without being rebuilt.

use std::io::Write;
use std::path::{Path, PathBuf};

use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use sha2::{Digest, Sha256};
use tar::{Archive, Builder, Header};

const MAGIC: &[u8; 8] = b"XMASPKG\0";
const TRAILER: usize = 16; // u64 payload_len (8) + magic (8)

// ─── public entry point ────────────────────────────────────────────────────────

/// If the running binary carries an embedded app payload, extract and run it, then
/// return Some(result). Returns None for a plain xmas binary.
pub fn try_run_embedded() -> Option<Result<(), String>> {
    let exe = std::env::current_exe().ok()?;
    let data = std::fs::read(&exe).ok()?;
    let payload = payload_slice(&data)?;
    Some(extract_and_run(payload))
}

// ─── used by `xmas compile` ─────────────────────────────────────────────────

/// Append `payload` (a tar.gz blob) and the 16-byte trailer to the binary at `exe_path`.
pub fn append_payload_to_exe(exe_path: &Path, payload: &[u8]) -> Result<(), String> {
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(exe_path)
        .map_err(|e| format!("open {}: {e}", exe_path.display()))?;
    f.write_all(payload).map_err(|e| e.to_string())?;
    let len = payload.len() as u64;
    f.write_all(&len.to_le_bytes()).map_err(|e| e.to_string())?;
    f.write_all(MAGIC).map_err(|e| e.to_string())?;
    Ok(())
}

/// Build a gzip-compressed tar archive from `main_js` bytes plus optional named assets.
/// Each asset path is stored relative to the extraction root (e.g. `"public/index.html"`).
pub fn make_payload(main_js: &[u8], assets: &[(String, Vec<u8>)]) -> Result<Vec<u8>, String> {
    let buf = Vec::new();
    let gz = GzEncoder::new(buf, Compression::best());
    let mut tar = Builder::new(gz);

    append_bytes(&mut tar, "main.js", main_js)?;
    for (name, data) in assets {
        append_bytes(&mut tar, name, data)?;
    }

    let gz = tar.into_inner().map_err(|e| e.to_string())?;
    gz.finish().map_err(|e| e.to_string())
}

// ─── internals ────────────────────────────────────────────────────────────────

fn append_bytes<W: Write>(tar: &mut Builder<W>, name: &str, data: &[u8]) -> Result<(), String> {
    let mut header = Header::new_gnu();
    header.set_size(data.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    tar.append_data(&mut header, name, data)
        .map_err(|e| e.to_string())
}

fn payload_slice(exe: &[u8]) -> Option<&[u8]> {
    if exe.len() < TRAILER {
        return None;
    }
    if &exe[exe.len() - 8..] != MAGIC {
        return None;
    }
    let len_bytes: [u8; 8] = exe[exe.len() - TRAILER..exe.len() - 8]
        .try_into()
        .ok()?;
    let len = u64::from_le_bytes(len_bytes) as usize;
    let start = exe.len().checked_sub(TRAILER + len)?;
    Some(&exe[start..exe.len() - TRAILER])
}

fn extract_and_run(payload: &[u8]) -> Result<(), String> {
    let hash = {
        let digest = Sha256::digest(payload);
        digest[..8]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    };

    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| "/tmp".into());
    let app_dir = PathBuf::from(home)
        .join(".xmas")
        .join("apps")
        .join(&hash);

    if !app_dir.join("main.js").exists() {
        extract_payload(payload, &app_dir)?;
    }

    std::env::set_current_dir(&app_dir)
        .map_err(|e| format!("chdir {}: {e}", app_dir.display()))?;

    crate::entry::run_file(&app_dir.join("main.js"))
}

fn extract_payload(payload: &[u8], out: &Path) -> Result<(), String> {
    std::fs::create_dir_all(out)
        .map_err(|e| format!("mkdir {}: {e}", out.display()))?;
    let gz = GzDecoder::new(payload);
    let mut archive = Archive::new(gz);
    archive
        .unpack(out)
        .map_err(|e| format!("unpack: {e}"))
}