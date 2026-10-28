use super::file_endpoint::{Endpoint, Metadata};
use super::file_tree::{self, Progress};
use std::collections::BTreeMap;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex, OnceLock,
};

struct Edit {
    owner: u64,
    endpoint: Endpoint,
    source: String,
    local: String,
    directory: tempfile::TempDir,
    metadata: Mutex<(Metadata, Metadata)>,
    busy: AtomicBool,
}
fn edits() -> &'static Mutex<BTreeMap<u64, Arc<Edit>>> {
    static EDITS: OnceLock<Mutex<BTreeMap<u64, Arc<Edit>>>> = OnceLock::new();
    EDITS.get_or_init(Default::default)
}

pub(super) async fn prepare(
    owner: u64,
    endpoint: &Endpoint,
    source: &str,
    progress: &Progress,
) -> Result<(String, u64), String> {
    if matches!(endpoint, Endpoint::Local) {
        return Err("remote editing requires an SFTP session".into());
    }
    let metadata = endpoint
        .metadata(source)
        .await?
        .ok_or("remote file no longer exists")?;
    if metadata.kind != 0 || metadata.size > 16 * 1024 * 1024 {
        return Err("remote editor supports regular files up to 16 MiB".into());
    }
    let directory = tempfile::Builder::new()
        .prefix("tideterm-edit-")
        .tempdir()
        .map_err(|e| e.to_string())?;
    let name = source
        .rsplit('/')
        .next()
        .ok_or("invalid remote file name")?;
    super::file_endpoint::safe_name(name)?;
    let local = directory.path().join(name).to_string_lossy().into_owned();
    file_tree::copy(endpoint, source, &Endpoint::Local, &local, 0, progress).await?;
    progress.check()?;
    endpoint.check()?;
    let local_metadata = Endpoint::Local
        .metadata(&local)
        .await?
        .ok_or("temporary file disappeared")?;
    if endpoint.metadata(source).await? != Some(metadata.clone()) {
        return Err("remote file changed during download".into());
    }
    let edit = Arc::new(Edit {
        owner,
        endpoint: endpoint.clone(),
        source: source.into(),
        local: local.clone(),
        directory,
        metadata: Mutex::new((metadata, local_metadata)),
        busy: AtomicBool::new(false),
    });
    let mut state = edits().lock().unwrap();
    if !super::local_files::files_exists(owner) {
        return Err("file browser closed".into());
    }
    progress.check()?;
    if state.len() >= 16 {
        return Err("remote edit limit reached".into());
    }
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    state.insert(id, edit);
    Ok((local, id))
}

pub(super) async fn upload(
    owner: u64,
    id: u64,
    progress: &Progress,
) -> Result<(String, u64), String> {
    let edit = edits()
        .lock()
        .unwrap()
        .get(&id)
        .filter(|e| e.owner == owner)
        .cloned()
        .ok_or("remote edit closed")?;
    if edit.busy.swap(true, Ordering::SeqCst) {
        return Err("remote edit already uploading".into());
    }
    let result = async {
        let previous = edit.metadata.lock().unwrap().0.clone();
        if edit.endpoint.metadata(&edit.source).await? != Some(previous.clone()) {
            return Err("remote file changed; refusing to overwrite it".into());
        }
        file_tree::copy_checked(
            &Endpoint::Local,
            &edit.local,
            &edit.endpoint,
            &edit.source,
            2,
            progress,
            Some(&previous),
        )
        .await?;
        let remote = edit
            .endpoint
            .metadata(&edit.source)
            .await?
            .ok_or("remote file disappeared after upload")?;
        let local = Endpoint::Local
            .metadata(&edit.local)
            .await?
            .ok_or("temporary file disappeared")?;
        *edit.metadata.lock().unwrap() = (remote, local);
        Ok((edit.local.clone(), id))
    }
    .await;
    edit.busy.store(false, Ordering::SeqCst);
    result
}

pub fn files_edit_close(owner: u64, id: u64, discard: bool) -> Result<(), String> {
    let mut state = edits().lock().unwrap();
    let edit = state
        .get(&id)
        .filter(|e| e.owner == owner)
        .ok_or("remote edit closed")?;
    if edit.busy.load(Ordering::SeqCst) {
        return Err("wait for the current upload to finish".into());
    }
    // Only discard this owned temporary directory after explicit confirmation.
    if !discard {
        return Err("confirm discarding the local editing copy before closing".into());
    }
    match std::fs::remove_dir_all(edit.directory.path()) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err("editor still owns the temporary file; close it and retry".into()),
    }
    state.remove(&id);
    Ok(())
}
pub(super) fn close_owner(owner: u64) {
    edits().lock().unwrap().retain(|_, e| e.owner != owner);
}
