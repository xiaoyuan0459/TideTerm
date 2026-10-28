//! Opt-in session directory suggestions. Workers never access the UI-thread tab registry.
use super::{
    command_completion::completion_valid,
    environment_provider::{eligible, Provider},
};
use crate::bridge::completion::completion_ffi::{
    CommandSuggestion, CompletionContext, EnvironmentBatch, ProgramSource,
};
use std::collections::{BTreeMap, VecDeque};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Condvar, Mutex, MutexGuard, OnceLock,
};
use std::time::{Duration, Instant};

#[derive(Clone)]
struct Request {
    id: u64,
    dialect: u8,
    programs: Option<ProgramSource>,
    context: CompletionContext,
    query: String,
    deadline: Instant,
    canceled: Arc<AtomicBool>,
}
#[derive(Clone)]
struct Cache {
    context: CompletionContext,
    dialect: u8,
    programs: Option<ProgramSource>,
    query: String,
    result: EnvironmentBatch,
    at: Instant,
}
#[derive(Default)]
struct Owner {
    request: Option<Request>,
    result: EnvironmentBatch,
    cache: Option<Cache>,
}
struct Job {
    owner: u64,
    request: Request,
    plan: Provider,
}
#[derive(Default)]
struct State {
    next: u64,
    owners: BTreeMap<u64, Owner>,
    jobs: VecDeque<Job>,
}
struct Worker {
    state: Mutex<State>,
    wake: Condvar,
}
impl Worker {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}
fn worker() -> &'static Worker {
    static WORKER: OnceLock<Worker> = OnceLock::new();
    WORKER.get_or_init(|| Worker {
        state: Mutex::new(State::default()),
        wake: Condvar::new(),
    })
}
fn response(state: u8) -> EnvironmentBatch {
    EnvironmentBatch {
        state,
        ..Default::default()
    }
}
pub fn environment_create() -> u64 {
    let mut state = worker().lock();
    if state.owners.len() == 16 {
        return 0;
    }
    state.next += 1;
    let id = state.next;
    state.owners.insert(id, Owner::default());
    id
}
fn start(remote: bool) -> bool {
    static STARTED: Mutex<[bool; 2]> = Mutex::new([false; 2]);
    let mut started = STARTED.lock().unwrap_or_else(|e| e.into_inner());
    let flag = &mut started[usize::from(remote)];
    if !*flag {
        *flag = std::thread::Builder::new()
            .name(if remote { "ssh-paths" } else { "local-paths" }.into())
            .spawn(move || work(remote))
            .is_ok();
    }
    *flag
}
pub fn environment_cancel(owner: u64) {
    if owner == 0 {
        return;
    }
    let mut state = worker().lock();
    state.jobs.retain(|job| job.owner != owner);
    if let Some(owner) = state.owners.get_mut(&owner) {
        if let Some(request) = owner.request.take() {
            request.canceled.store(true, Ordering::Relaxed);
        }
        owner.result = response(0);
    }
}
pub fn environment_close(owner: u64) {
    environment_cancel(owner);
    if owner != 0 {
        worker().lock().owners.remove(&owner);
    }
}
pub fn environment_request(
    owner: u64,
    context: &CompletionContext,
    query: &str,
    dialect: u8,
) -> EnvironmentBatch {
    begin(owner, context, query, dialect, None)
}
pub fn programs_request(
    owner: u64,
    context: &CompletionContext,
    query: &str,
    dialect: u8,
    settings: &ProgramSource,
) -> EnvironmentBatch {
    if !super::program_source::prefix(query) {
        environment_cancel(owner);
        return response(0);
    }
    begin(owner, context, query, dialect, Some(settings))
}
fn begin(
    owner: u64,
    context: &CompletionContext,
    query: &str,
    dialect: u8,
    programs: Option<&ProgramSource>,
) -> EnvironmentBatch {
    environment_cancel(owner);
    if !completion_valid(context) {
        return response(9);
    }
    let plan = match if let Some(settings) = programs {
        super::program_source::Plan::capture(context, query, dialect, settings)
            .map(Provider::Programs)
    } else {
        Provider::capture(context.source, &context.cwd, query, dialect)
    } {
        Ok(plan) => plan,
        Err(state) => return response(state),
    };
    let remote = plan.remote();
    let mut state = worker().lock();
    if !state.owners.contains_key(&owner) {
        return response(8);
    }
    if !start(remote) {
        return response(8);
    }
    state.next += 1;
    let request = Request {
        id: state.next,
        dialect,
        programs: programs.cloned(),
        context: context.clone(),
        query: query.into(),
        deadline: Instant::now() + Duration::from_secs(if remote { 5 } else { 1 }),
        canceled: Arc::new(AtomicBool::new(false)),
    };
    let current = state
        .owners
        .get_mut(&owner)
        .expect("owner checked under the same lock");
    let cached = current
        .cache
        .as_ref()
        .filter(|c| {
            c.context == *context
                && c.query == query
                && c.dialect == dialect
                && c.programs.as_ref() == programs
                && c.at.elapsed() < Duration::from_secs(2)
        })
        .map(|c| c.result.clone());
    current.request = Some(request.clone());
    current.result = cached.unwrap_or_else(|| response(1));
    current.result.request = request.id;
    let result = current.result.clone();
    if result.state == 1 {
        state.jobs.push_back(Job {
            owner,
            request,
            plan,
        });
        worker().wake.notify_all();
    }
    result
}
pub fn environment_poll(owner: u64) -> EnvironmentBatch {
    let request = worker()
        .lock()
        .owners
        .get(&owner)
        .and_then(|o| o.request.clone());
    let Some(request) = request else {
        return response(0);
    };
    let valid = completion_valid(&request.context) && eligible(request.context.source);
    let mut state = worker().lock();
    let Some(current) = state
        .owners
        .get_mut(&owner)
        .filter(|o| o.request.as_ref().is_some_and(|r| r.id == request.id))
    else {
        return response(0);
    };
    if !valid || (current.result.state == 1 && Instant::now() >= request.deadline) {
        request.canceled.store(true, Ordering::Relaxed);
        current.result = response(if valid { 7 } else { 9 });
        current.result.request = request.id;
    }
    current.result.clone()
}
pub(super) fn ready_items(
    owner: u64,
    context: &CompletionContext,
    query: &str,
) -> Vec<CommandSuggestion> {
    if owner == 0 || !eligible(context.source) {
        return vec![];
    }
    let state = worker().lock();
    state
        .owners
        .get(&owner)
        .filter(|o| {
            o.result.state == 2
                && o.request
                    .as_ref()
                    .is_some_and(|r| r.context == *context && r.query == query)
        })
        .map(|o| o.result.items.clone())
        .unwrap_or_default()
}
pub fn environment_valid(
    owner: u64,
    request: u64,
    context: &CompletionContext,
    query: &str,
) -> bool {
    if !completion_valid(context) || !eligible(context.source) {
        return false;
    }
    worker().lock().owners.get(&owner).is_some_and(|o| {
        o.result.state == 2
            && o.request
                .as_ref()
                .is_some_and(|r| r.id == request && r.context == *context && r.query == query)
    })
}
fn work(remote: bool) {
    loop {
        let job = {
            let mut state = worker().lock();
            loop {
                if let Some(at) = state
                    .jobs
                    .iter()
                    .position(|job| job.plan.remote() == remote)
                {
                    break state
                        .jobs
                        .remove(at)
                        .expect("queue index found under the same lock");
                }
                state = worker().wake.wait(state).unwrap_or_else(|e| e.into_inner());
            }
        };
        let mut result = job
            .plan
            .scan(job.request.deadline, job.request.canceled.clone());
        if Instant::now() >= job.request.deadline {
            result = response(7);
        }
        result.request = job.request.id;
        let mut state = worker().lock();
        if let Some(owner) = state.owners.get_mut(&job.owner).filter(|o| {
            o.result.state == 1
                && o.request
                    .as_ref()
                    .is_some_and(|r| r.id == job.request.id && !r.canceled.load(Ordering::Relaxed))
        }) {
            if result.state == 2 {
                owner.cache = Some(Cache {
                    context: job.request.context,
                    dialect: job.request.dialect,
                    programs: job.request.programs,
                    query: job.request.query,
                    result: result.clone(),
                    at: Instant::now(),
                });
            }
            owner.result = result;
        }
    }
}

#[cfg(test)]
#[path = "command_environment_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "environment_remote_tests.rs"]
mod remote_tests;

#[cfg(test)]
#[path = "environment_quoted_tests.rs"]
mod quoted_tests;
