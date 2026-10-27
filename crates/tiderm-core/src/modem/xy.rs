//! XMODEM checksum/CRC and YMODEM batch (128/1024-byte blocks).
use super::{
    files::{safe_name, Download},
    wire::Wire,
    Kind,
};
use std::{
    io::Read,
    path::{Path, PathBuf},
};
const SOH: u8 = 1;
const STX: u8 = 2;
const EOT: u8 = 4;
const ACK: u8 = 6;
const NAK: u8 = 21;
const CAN: u8 = 24;
const RETRIES: usize = 10;
pub(super) fn crc(bytes: &[u8]) -> u16 {
    let mut crc = 0u16;
    for byte in bytes {
        crc ^= (*byte as u16) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x1021
            } else {
                crc << 1
            };
        }
    }
    crc
}
async fn control(wire: &mut Wire, choices: &[u8]) -> Result<u8, String> {
    for _ in 0..65536 {
        let byte = wire.byte().await?;
        if byte == CAN {
            return Err("peer cancelled transfer".into());
        }
        if choices.contains(&byte) {
            return Ok(byte);
        }
    }
    Err("too much non-protocol output".into())
}
async fn packet(wire: &mut Wire, number: u8, data: &[u8], crc_mode: bool) -> Result<(), String> {
    let mut packet = vec![if data.len() == 1024 { STX } else { SOH }, number, !number];
    packet.extend_from_slice(data);
    if crc_mode {
        packet.extend(crc(data).to_be_bytes());
    } else {
        packet.push(data.iter().fold(0u8, |sum, byte| sum.wrapping_add(*byte)));
    }
    for _ in 0..RETRIES {
        wire.write(&packet).await?;
        match control(wire, &[ACK, NAK]).await {
            Ok(ACK) => return Ok(()),
            Ok(_) => {}
            Err(error) if error == "Modem response timed out" => {}
            Err(error) => return Err(error),
        }
    }
    Err("block retry limit exceeded".into())
}
async fn finish_send(wire: &mut Wire, y: bool) -> Result<(), String> {
    for _ in 0..RETRIES {
        wire.write(&[EOT]).await?;
        match control(wire, &[ACK, NAK]).await {
            Ok(ACK) => {
                if y {
                    control(wire, b"C").await?;
                }
                return Ok(());
            }
            Ok(_) => {}
            Err(e) if e == "Modem response timed out" => {}
            Err(e) => return Err(e),
        }
    }
    Err("end-of-file retry limit exceeded".into())
}
pub async fn send(wire: &mut Wire, kind: Kind, paths: &[PathBuf]) -> Result<(), String> {
    let y = matches!(kind, Kind::Y);
    if paths.is_empty() || paths.len() > 1024 || (!y && paths.len() != 1) {
        return Err("invalid file count for Modem protocol".into());
    }
    let crc_mode = control(wire, &[b'C', NAK]).await? == b'C';
    if y && !crc_mode {
        return Err("YMODEM requires CRC mode".into());
    }
    let mut total = 0;
    for path in paths {
        let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
        let size = file.metadata().map_err(|e| e.to_string())?.len();
        let name = safe_name(
            path.file_name()
                .and_then(|n| n.to_str())
                .ok_or("invalid file name")?
                .as_bytes(),
        )?;
        if y {
            let metadata = format!("{name}\0{size}\0");
            if metadata.len() > 128 {
                return Err("YMODEM file header exceeds 128 bytes".into());
            }
            let mut header = [0; 128];
            header[..metadata.len()].copy_from_slice(metadata.as_bytes());
            packet(wire, 0, &header, true).await?;
            control(wire, b"C").await?;
        }
        let mut number = 1u8;
        let mut sent = 0;
        while sent < size {
            let count = (size - sent).min(1024) as usize;
            let block_size = if count <= 128 { 128 } else { 1024 };
            let mut block = vec![0x1a; block_size];
            file.read_exact(&mut block[..count])
                .map_err(|e| e.to_string())?;
            packet(wire, number, &block, crc_mode).await?;
            number = number.wrapping_add(1);
            sent += count as u64;
            total += count as u64;
            wire.router.update(total, &name);
        }
        finish_send(wire, y).await?;
    }
    if y {
        packet(wire, 0, &[0; 128], true).await?;
    }
    Ok(())
}
enum Block {
    Data(u8, Vec<u8>),
    End,
}
async fn block(wire: &mut Wire, crc_mode: bool) -> Result<Block, String> {
    let start = control(wire, &[SOH, STX, EOT]).await?;
    if start == EOT {
        return Ok(Block::End);
    }
    let header = wire.exact(2).await?;
    let data = wire.exact(if start == SOH { 128 } else { 1024 }).await?;
    let trailer = wire.exact(if crc_mode { 2 } else { 1 }).await?;
    let valid = if crc_mode {
        crc(&data) == u16::from_be_bytes([trailer[0], trailer[1]])
    } else {
        data.iter().fold(0u8, |sum, byte| sum.wrapping_add(*byte)) == trailer[0]
    };
    if header[0] != !header[1] || !valid {
        return Err("invalid block checksum".into());
    }
    Ok(Block::Data(header[0], data))
}
async fn next(wire: &mut Wire, request: u8, crc_mode: bool) -> Result<Block, String> {
    for _ in 0..RETRIES {
        match block(wire, crc_mode).await {
            Ok(b) => return Ok(b),
            Err(e) if e == "Modem response timed out" || e == "invalid block checksum" => {
                wire.write(&[request]).await?
            }
            Err(e) => return Err(e),
        }
    }
    Err("receive retry limit exceeded".into())
}
async fn first_x(wire: &mut Wire) -> Result<(Block, bool), String> {
    // Prefer CRC, then negotiate the original checksum variant with older peers.
    for attempt in 0..RETRIES {
        let crc_mode = attempt < 3;
        wire.write(if crc_mode { b"C" } else { &[NAK] }).await?;
        match block(wire, crc_mode).await {
            Ok(block) => return Ok((block, crc_mode)),
            Err(e) if e == "Modem response timed out" || e == "invalid block checksum" => {}
            Err(e) => return Err(e),
        }
    }
    Err("receive handshake retry limit exceeded".into())
}
pub async fn receive(wire: &mut Wire, kind: Kind, path: &Path) -> Result<(), String> {
    let y = matches!(kind, Kind::Y);
    if y && !path.is_dir() {
        return Err("YMODEM receive requires an existing directory".into());
    }
    let mut total = 0;
    if y {
        wire.write(b"C").await?;
    }
    for _ in 0..1024 {
        let (destination, size) = if y {
            let Block::Data(0, header) = next(wire, b'C', true).await? else {
                return Err("expected YMODEM file header".into());
            };
            if header[0] == 0 {
                wire.write(&[ACK]).await?;
                return Ok(());
            }
            let name_end = header
                .iter()
                .position(|b| *b == 0)
                .ok_or("unterminated file name")?;
            let name = safe_name(&header[..name_end])?;
            let size_end = header[name_end + 1..]
                .iter()
                .position(|b| *b == 0 || *b == b' ')
                .unwrap_or(header.len() - name_end - 1)
                + name_end
                + 1;
            let size = std::str::from_utf8(&header[name_end + 1..size_end])
                .ok()
                .and_then(|n| n.parse::<u64>().ok())
                .ok_or("invalid YMODEM file size")?;
            (path.join(name), Some(size))
        } else {
            (path.to_path_buf(), None)
        };
        let mut file = Download::new(&destination, size)?;
        let name = destination
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        if y {
            wire.write(&[ACK, b'C']).await?;
        }
        let mut expected = 1u8;
        let (mut first, crc_mode) = if y {
            (None, true)
        } else {
            let (block, crc_mode) = first_x(wire).await?;
            (Some(block), crc_mode)
        };
        loop {
            let block = match first.take() {
                Some(block) => block,
                None => next(wire, NAK, crc_mode).await?,
            };
            match block {
                Block::End => {
                    if y {
                        wire.write(&[NAK]).await?;
                        if !matches!(next(wire, NAK, true).await?, Block::End) {
                            return Err("expected second EOT".into());
                        }
                    }
                    file.finish()?;
                    wire.write(if y { &[ACK, b'C'] } else { &[ACK] }).await?;
                    break;
                }
                Block::Data(number, data) if number == expected => {
                    if file.size.is_some_and(|size| file.written >= size) {
                        return Err("data beyond declared file size".into());
                    }
                    total += file.write(&data)? as u64;
                    wire.router.update(total, &name);
                    expected = expected.wrapping_add(1);
                    wire.write(&[ACK]).await?;
                }
                Block::Data(number, _) if number == expected.wrapping_sub(1) => {
                    wire.write(if y && expected == 1 {
                        &[ACK, b'C']
                    } else {
                        &[ACK]
                    })
                    .await?;
                }
                _ => return Err("unexpected block sequence".into()),
            }
        }
        if !y {
            return Ok(());
        }
    }
    Err("batch file limit exceeded".into())
}
