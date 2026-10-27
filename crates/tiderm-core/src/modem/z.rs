//! Drive the dependency's bounded ZMODEM machine through the same raw stream.
use super::{
    files::{safe_name, Download},
    wire::Wire,
};
use std::{
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};
use zmodem2::{Action, Event, FileInfo, Position, Receiver, Sender};
fn error(e: zmodem2::Error) -> String {
    e.to_string()
}
pub async fn send(wire: &mut Wire, paths: &[PathBuf]) -> Result<(), String> {
    if paths.is_empty() || paths.len() > 1024 {
        return Err("invalid ZMODEM file count".into());
    }
    let mut sender = Sender::new().map_err(error)?;
    let mut index: usize = 0;
    let mut file = None;
    let mut finishing = false;
    let mut completed = false;
    let mut pending = Vec::new();
    let mut total = 0;
    let mut retries = 0;
    loop {
        wire.check()?;
        match sender.poll() {
            Action::WriteWire(bytes) => {
                let n = bytes.len();
                wire.write(bytes).await?;
                sender.wire_written(n);
            }
            Action::ReadFile { offset, max_len } => {
                let file: &mut std::fs::File = file
                    .as_mut()
                    .ok_or("ZMODEM requested data without a file")?;
                file.seek(SeekFrom::Start(offset.get().into()))
                    .map_err(|e| e.to_string())?;
                let mut bytes = vec![0; max_len];
                let n = file.read(&mut bytes).map_err(|e| e.to_string())?;
                sender.submit_file(&bytes[..n]).map_err(error)?;
                wire.router.update(
                    total + offset.get() as u64 + n as u64,
                    paths[index.saturating_sub(1)]
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .as_ref(),
                );
            }
            Action::Event(Event::FileCompleted) => {
                if let Some(f) = file.take() {
                    total += f.metadata().map_err(|e| e.to_string())?.len();
                }
            }
            Action::Event(Event::SessionCompleted) => completed = true,
            Action::Event(Event::Aborted) => return Err("peer aborted ZMODEM transfer".into()),
            Action::Idle => {
                if completed {
                    wire.unread(&pending);
                    return Ok(());
                }
                if file.is_none() && !finishing {
                    if let Some(path) = paths.get(index) {
                        let f = std::fs::File::open(path).map_err(|e| e.to_string())?;
                        let size = u32::try_from(f.metadata().map_err(|e| e.to_string())?.len())
                            .map_err(|_| "ZMODEM file exceeds 4 GiB")?;
                        let name = safe_name(
                            path.file_name()
                                .and_then(|n| n.to_str())
                                .ok_or("invalid file name")?
                                .as_bytes(),
                        )?;
                        // The machine can still be negotiating ZRINIT; defer offering.
                        if sender
                            .start_file(FileInfo::new(name.as_bytes(), Some(Position::new(size))))
                            .is_ok()
                        {
                            file = Some(f);
                            index += 1;
                            continue;
                        }
                    } else if sender.finish().is_ok() {
                        finishing = true;
                        continue;
                    }
                }
                if pending.is_empty() {
                    match wire.chunk().await {
                        Ok(bytes) => {
                            pending = bytes;
                            retries = 0;
                        }
                        Err(e) if e == "Modem response timed out" && retries < 10 => {
                            retries += 1;
                            sender.timeout().map_err(error)?;
                            continue;
                        }
                        Err(e) => return Err(e),
                    }
                }
                let used = sender.submit_wire(&pending).map_err(error)?;
                if used == 0 {
                    return Err("ZMODEM input made no progress".into());
                }
                pending.drain(..used);
            }
            _ => {}
        }
    }
}
pub async fn receive(wire: &mut Wire, directory: &Path) -> Result<(), String> {
    if !directory.is_dir() {
        return Err("ZMODEM receive requires an existing directory".into());
    }
    let mut receiver = Receiver::new().map_err(error)?;
    receiver.set_manual_file_accept(true);
    let mut file: Option<Download> = None;
    let mut name = String::new();
    let mut pending = Vec::new();
    let mut total = 0;
    let mut retries = 0;
    let mut count = 0;
    let mut completed = false;
    loop {
        wire.check()?;
        match receiver.poll() {
            Action::WriteWire(bytes) => {
                let n = bytes.len();
                wire.write(bytes).await?;
                receiver.wire_written(n);
            }
            Action::Event(Event::FileStarted(info)) => {
                count += 1;
                if count > 1024 {
                    return Err("batch file limit exceeded".into());
                }
                name = safe_name(info.name)?;
                file = Some(Download::new(
                    &directory.join(&name),
                    info.size.map(|n| n.get().into()),
                )?);
                receiver.accept_file_at(0).map_err(error)?;
            }
            Action::WriteFile(bytes) => {
                let f = file.as_mut().ok_or("ZMODEM data without a file")?;
                if f.size
                    .is_some_and(|size| f.written + bytes.len() as u64 > size)
                {
                    return Err("data beyond declared file size".into());
                }
                let n = f.write(bytes)?;
                total += n as u64;
                receiver.file_written(n).map_err(error)?;
                wire.router.update(total, &name);
            }
            Action::Event(Event::FileCompleted) => {
                file.take()
                    .ok_or("ZMODEM completion without a file")?
                    .finish()?;
            }
            Action::Event(Event::SessionCompleted) => completed = true,
            Action::Event(Event::Aborted) => return Err("peer aborted ZMODEM transfer".into()),
            Action::Idle => {
                if completed {
                    wire.unread(&pending);
                    return Ok(());
                }
                if pending.is_empty() {
                    match wire.chunk().await {
                        Ok(bytes) => {
                            pending = bytes;
                            retries = 0;
                        }
                        Err(e) if e == "Modem response timed out" && retries < 10 => {
                            retries += 1;
                            receiver.timeout().map_err(error)?;
                            continue;
                        }
                        Err(e) => return Err(e),
                    }
                }
                let used = receiver.submit_wire(&pending).map_err(error)?;
                if used == 0 {
                    return Err("ZMODEM input made no progress".into());
                }
                pending.drain(..used);
            }
            _ => {}
        }
    }
}
