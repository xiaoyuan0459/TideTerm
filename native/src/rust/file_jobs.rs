use super::file_endpoint::Endpoint;
use super::file_tree::{self, Progress};
use crate::bridge::files::files_ffi::FileTask;
use std::collections::BTreeMap;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex, OnceLock,
};

type Outcome = Result<(String, u64), String>;
pub fn files_quote_path(path: &str, dialect: u8) -> Result<String, String> {
    if path.len() > 4096 || !matches!(dialect, 1 | 2) {
        return Err("不支持的路径或 Shell 语法".into());
    }
    super::environment_draft::word(path, dialect, false, true)
        .map(|(value, _)| value)
        .ok_or_else(|| "该路径不能按所选 Shell 语法安全填入".into())
}
struct Job {
    owner: u64,
    progress: Progress,
    result: Mutex<Option<Outcome>>,
}
fn jobs() -> &'static Mutex<BTreeMap<u64, Arc<Job>>> {
    static JOBS: OnceLock<Mutex<BTreeMap<u64, Arc<Job>>>> = OnceLock::new();
    JOBS.get_or_init(Default::default)
}

pub fn files_task_start(
    owner: u64,
    op: u8,
    source_uid: &str,
    source: &str,
    target_uid: &str,
    target: &str,
    policy: u8,
) -> Result<u64, String> {
    if super::term_ops::input_locked() {
        return Err("file operation blocked while locked".into());
    }
    if !super::local_files::files_exists(owner) {
        return Err("file browser closed".into());
    }
    if op > 7 || policy > 2 {
        return Err("invalid file operation".into());
    }
    let src = Endpoint::for_uid(source_uid)?;
    let dst = Endpoint::for_uid(target_uid)?;
    if !matches!(op, 0 | 1 | 7) {
        file_tree::validate_path(&src, source)?;
    }
    if !matches!(op, 3 | 6 | 7) {
        file_tree::validate_path(&dst, target)?;
    }
    if op == 2 && source_uid != target_uid {
        return Err("move must stay within one filesystem".into());
    }
    let source = source.to_owned();
    let target = target.to_owned();
    if op == 4
        && source_uid == target_uid
        && (target == source || target.starts_with(&format!("{}/", source.trim_end_matches('/'))))
    {
        return Err("destination must not be inside the source".into());
    }
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let job = Arc::new(Job {
        owner,
        progress: Progress::default(),
        result: Mutex::new(None),
    });
    {
        let mut state = jobs().lock().unwrap();
        if !super::local_files::files_exists(owner) {
            return Err("file browser closed".into());
        }
        if state.len() >= 64 || state.values().filter(|j| j.owner == owner).count() >= 16 {
            return Err("file operation queue is full".into());
        }
        state.insert(id, job.clone());
    }
    super::session::shared_runtime().spawn(async move {
        static PERMITS: OnceLock<tokio::sync::Semaphore> = OnceLock::new();
        let _permit = PERMITS
            .get_or_init(|| tokio::sync::Semaphore::new(4))
            .acquire()
            .await;
        let result = async {
            job.progress.check()?;
            match op {
                0 => dst.mkdir(&target).await?,
                1 => dst.create_file(&target).await?,
                2 => {
                    if dst.metadata(&target).await?.is_some() {
                        return Err("move destination already exists".into());
                    }
                    src.rename_new(&source, &target).await?;
                }
                3 => file_tree::remove(&src, &source, &job.progress).await?,
                4 => file_tree::copy(&src, &source, &dst, &target, policy, &job.progress).await?,
                5 => dst.symlink(&source, &target).await?,
                6 => return super::remote_edit::prepare(owner, &src, &source, &job.progress).await,
                7 => {
                    return super::remote_edit::upload(
                        owner,
                        source.parse().map_err(|_| "invalid edit handle")?,
                        &job.progress,
                    )
                    .await
                }
                _ => unreachable!(),
            }
            Ok((target, 0))
        }
        .await;
        *job.result.lock().unwrap() = Some(result);
    });
    Ok(id)
}

pub fn files_task_poll(owner: u64, id: u64) -> FileTask {
    let mut state = jobs().lock().unwrap();
    let Some(job) = state.get(&id).filter(|j| j.owner == owner).cloned() else {
        return FileTask {
            id,
            edit: 0,
            state: 0,
            done: 0,
            total: 0,
            bytes: 0,
            path: String::new(),
            error: String::new(),
        };
    };
    let result = job.result.lock().unwrap().clone();
    let (status, path, edit, error) = match result {
        None => (
            1,
            job.progress.current.lock().unwrap().clone(),
            0,
            String::new(),
        ),
        Some(Ok((path, edit))) => (2, path, edit, String::new()),
        Some(Err(error)) => (
            if job.progress.cancel.load(Ordering::Relaxed) {
                4
            } else {
                3
            },
            String::new(),
            0,
            error,
        ),
    };
    if status != 1 {
        state.remove(&id);
    }
    FileTask {
        id,
        edit,
        state: status,
        done: job.progress.done.load(Ordering::Relaxed),
        total: job.progress.total.load(Ordering::Relaxed),
        bytes: job.progress.bytes.load(Ordering::Relaxed),
        path,
        error,
    }
}

pub fn files_task_cancel(owner: u64, id: u64) {
    if let Some(job) = jobs().lock().unwrap().get(&id).filter(|j| j.owner == owner) {
        job.progress.cancel.store(true, Ordering::Relaxed);
    }
}
pub(super) fn close(owner: u64) {
    super::remote_edit::close_owner(owner);
    jobs().lock().unwrap().retain(|_, job| {
        if job.owner != owner {
            return true;
        }
        job.progress.cancel.store(true, Ordering::Relaxed);
        false
    });
}
