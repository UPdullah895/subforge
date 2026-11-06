//! Job scheduler — the piece every later phase leans on.
//!
//! Properties it guarantees, in the order they matter for a media server:
//!
//! 1. **One heavy job at a time** by default (transcription, translation, batch, model
//!    download). A second one waits with the stage `waiting_for_slot` instead of competing
//!    for the GPU. Light jobs (probe, mux, convert, retime, clean) have their own budget and
//!    may run alongside a heavy one.
//! 2. **Priority**: a job the user started outranks one an automation source queued, without
//!    starving it (users first, then FIFO).
//! 3. **One writer per file**: two jobs never touch the same path at the same time, whatever
//!    their class — the second waits with the stage `waiting_for_file`.
//! 4. **A truthful journal**: every transition is appended to `<config dir>/server/jobs.jsonl`,
//!    so a restart does not silently forget what was running. Interrupted jobs come back as
//!    `failed` with a message saying so, rather than looking like they never happened.
//!
//! The scheduler deliberately does not own the body of a job: routes keep their logic and
//! their own progress reporting, and hand the future to [`Scheduler::gated`]. That keeps the
//! change small and lets the pipeline (P2) reuse the same gate for its steps.

use std::collections::HashMap;
use std::future::Future;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tokio::sync::{oneshot, Mutex as AsyncMutex};

use super::{emit_event, EventSender, JobEvent, JobStatus, JobType, SharedJobQueue};

/// How much of the machine a job may take.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobClass {
    Heavy,
    Light,
}

/// Who asked for the job. Ranked in [`JobSource::rank`]; lower runs first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobSource {
    User,
    Automation,
}

impl JobSource {
    fn rank(self) -> u8 {
        match self {
            JobSource::User => 0,
            JobSource::Automation => 1,
        }
    }
}

/// Transcription, translation, batch runs and model downloads are the jobs that saturate a
/// GPU or a disk; everything else is short and cheap.
pub fn class_of(kind: &JobType) -> JobClass {
    use JobType::*;
    match kind {
        Transcribe | Translate | BatchOperation | DownloadModel => JobClass::Heavy,
        ExtractSubtitle | ExtractAudio | MergeSubtitle | MergeAudio | RemoveTracks
        | ConvertSubtitle | ShiftSubtitle => JobClass::Light,
    }
}

/// What `/api/jobs` shows for a job the scheduler knows about.
#[derive(Debug, Clone, Serialize)]
pub struct JobView {
    #[serde(flatten)]
    pub job: super::Job,
    pub class: JobClass,
    pub source: JobSource,
    pub stage: Option<String>,
}

/// One line of the journal. Last record for an id wins.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct JournalRecord {
    id: String,
    job_type: JobType,
    status: JobStatus,
    input_file: String,
    output_file: Option<String>,
    class: JobClass,
    source: JobSource,
    stage: Option<String>,
    message: Option<String>,
    updated_at: u64,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ── Priority gate ───────────────────────────────────────────────────────────────────────────

struct GateWaiter {
    rank: u8,
    seq: u64,
    slot: Option<oneshot::Sender<()>>,
}

struct GateState {
    running: usize,
    waiters: Vec<GateWaiter>,
}

/// A capacity limiter that hands free slots to the best-ranked waiter first.
struct Gate {
    capacity: usize,
    state: Mutex<GateState>,
}

impl Gate {
    fn new(capacity: usize) -> Arc<Self> {
        Arc::new(Self {
            capacity: capacity.max(1),
            state: Mutex::new(GateState {
                running: 0,
                waiters: Vec::new(),
            }),
        })
    }

    async fn acquire(self: &Arc<Self>, rank: u8, seq: u64) -> GatePermit {
        let wait = {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            if state.running < self.capacity && state.waiters.is_empty() {
                state.running += 1;
                None
            } else {
                let (tx, rx) = oneshot::channel();
                state.waiters.push(GateWaiter {
                    rank,
                    seq,
                    slot: Some(tx),
                });
                Some(rx)
            }
        };
        if let Some(rx) = wait {
            // The sender is only dropped without sending if the gate is torn down.
            let _ = rx.await;
        }
        GatePermit { gate: self.clone() }
    }

    fn release(&self) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.running = state.running.saturating_sub(1);
        let capacity = self.capacity;
        while state.running < capacity {
            let next = state
                .waiters
                .iter()
                .enumerate()
                .min_by_key(|(_, waiter)| (waiter.rank, waiter.seq))
                .map(|(index, _)| index);
            let Some(index) = next else { break };
            let mut waiter = state.waiters.remove(index);
            state.running += 1;
            if let Some(tx) = waiter.slot.take() {
                let _ = tx.send(());
            }
        }
    }

    #[cfg(test)]
    fn running(&self) -> usize {
        self.state.lock().unwrap().running
    }
}

struct GatePermit {
    gate: Arc<Gate>,
}

impl Drop for GatePermit {
    fn drop(&mut self) {
        self.gate.release();
    }
}

// ── Scheduler ───────────────────────────────────────────────────────────────────────────────

#[derive(Clone)]
struct Meta {
    kind: JobType,
    class: JobClass,
    source: JobSource,
    stage: String,
    input_file: String,
    output_file: Option<String>,
}

impl Meta {
    fn record(
        &self,
        id: &str,
        status: JobStatus,
        stage: &str,
        message: Option<String>,
    ) -> JournalRecord {
        JournalRecord {
            id: id.to_string(),
            job_type: self.kind.clone(),
            status,
            input_file: self.input_file.clone(),
            output_file: self.output_file.clone(),
            class: self.class,
            source: self.source,
            stage: Some(stage.to_string()),
            message,
            updated_at: now(),
        }
    }
}

struct Inner {
    queue: SharedJobQueue,
    events: EventSender,
    heavy: Arc<Gate>,
    light: Arc<Gate>,
    file_locks: Mutex<HashMap<String, Arc<AsyncMutex<()>>>>,
    meta: Mutex<HashMap<String, Meta>>,
    journal: Option<PathBuf>,
    seq: AtomicU64,
}

/// Handle to the scheduler; cheap to clone, shared through `AppState`.
#[derive(Clone)]
pub struct Scheduler {
    inner: Arc<Inner>,
}

impl Scheduler {
    /// Builds a scheduler and recovers the journal in `state_dir` (see [`default_state_dir`]).
    pub fn new(queue: SharedJobQueue, events: EventSender) -> Self {
        Self::with_journal(queue, events, default_state_dir())
    }

    /// Same, with an explicit journal directory — tests use a temp dir.
    pub fn with_journal(
        queue: SharedJobQueue,
        events: EventSender,
        state_dir: Option<PathBuf>,
    ) -> Self {
        let heavy_limit = env_limit("SUBFORGE_MAX_HEAVY_JOBS", 1);
        let light_limit = env_limit("SUBFORGE_MAX_LIGHT_JOBS", 2);

        let journal = state_dir.map(|dir| {
            let _ = std::fs::create_dir_all(&dir);
            dir.join("jobs.jsonl")
        });

        let scheduler = Self {
            inner: Arc::new(Inner {
                queue,
                events,
                heavy: Gate::new(heavy_limit),
                light: Gate::new(light_limit),
                file_locks: Mutex::new(HashMap::new()),
                meta: Mutex::new(HashMap::new()),
                journal,
                seq: AtomicU64::new(1),
            }),
        };
        scheduler.recover();
        scheduler
    }

    /// Every job the scheduler knows about, for `GET /api/jobs`.
    pub fn snapshot(&self) -> Vec<JobView> {
        let queue = self.inner.queue.lock().unwrap_or_else(|e| e.into_inner());
        let meta = self.inner.meta.lock().unwrap_or_else(|e| e.into_inner());
        queue
            .jobs
            .iter()
            .map(|job| {
                let known = meta.get(&job.id);
                JobView {
                    job: job.clone(),
                    class: known
                        .map(|m| m.class)
                        .unwrap_or_else(|| class_of(&job.job_type)),
                    source: known.map(|m| m.source).unwrap_or(JobSource::User),
                    stage: known.map(|m| m.stage.clone()),
                }
            })
            .collect()
    }

    /// The stage a job is in (`queued`, `waiting_for_file`, `waiting_for_slot`, `running`, `done`).
    pub fn stage_of(&self, id: &str) -> Option<String> {
        let meta = self.inner.meta.lock().unwrap_or_else(|e| e.into_inner());
        meta.get(id).map(|m| m.stage.clone())
    }

    /// Runs `body` behind the gate. Returns at once; the job itself is a spawned task.
    ///
    /// The caller keeps ownership of the job's status and progress (routes already report
    /// both); the scheduler adds the waiting stages, the per-file lock and the journal.
    pub fn gated<F>(&self, job_id: String, kind: JobType, source: JobSource, file: String, body: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let inner = self.inner.clone();
        let class = class_of(&kind);
        let seq = inner.seq.fetch_add(1, Ordering::SeqCst);
        let rank = source.rank();

        inner.register(
            &job_id,
            Meta {
                kind: kind.clone(),
                class,
                source,
                stage: "queued".to_string(),
                input_file: file.clone(),
                output_file: None,
            },
        );

        tokio::spawn(async move {
            let lock = inner.file_lock(&file);
            if lock.try_lock().is_err() {
                inner.set_stage(&job_id, "waiting_for_file");
            }
            // Held for the whole job: one writer per path.
            let _file_guard = lock.lock().await;

            inner.set_stage(&job_id, "waiting_for_slot");
            let gate = match class {
                JobClass::Heavy => inner.heavy.clone(),
                JobClass::Light => inner.light.clone(),
            };
            let _slot = gate.acquire(rank, seq).await;

            inner.set_stage(&job_id, "running");
            body.await;

            inner.finish(&job_id);
        });
    }

    // ── journal ─────────────────────────────────────────────────────────────────────────────

    fn recover(&self) {
        let Some(path) = self.inner.journal.clone() else {
            return;
        };
        for record in read_journal(&path) {
            // A queued or running job from a previous process cannot be revived from a generic
            // closure, so say what happened instead of pretending it is still going. Pipeline
            // jobs (P2) are declarative and their own handler re-submits them.
            let interrupted = matches!(
                record.status,
                JobStatus::Pending | JobStatus::Running | JobStatus::Paused
            );
            let (status, stage, message) = if interrupted {
                (
                    JobStatus::Failed,
                    "interrupted".to_string(),
                    Some("the server restarted while this job was queued or running".to_string()),
                )
            } else {
                (
                    record.status.clone(),
                    record
                        .stage
                        .clone()
                        .unwrap_or_else(|| "unknown".to_string()),
                    record.message.clone(),
                )
            };

            {
                let mut queue = self.inner.queue.lock().unwrap_or_else(|e| e.into_inner());
                if queue.get(&record.id).is_none() {
                    queue.add(super::Job {
                        id: record.id.clone(),
                        job_type: record.job_type.clone(),
                        status: status.clone(),
                        input_file: record.input_file.clone(),
                        output_file: record.output_file.clone(),
                        progress: 0.0,
                        message: message.clone(),
                    });
                }
            }
            {
                let mut meta = self.inner.meta.lock().unwrap_or_else(|e| e.into_inner());
                meta.insert(
                    record.id.clone(),
                    Meta {
                        kind: record.job_type.clone(),
                        class: record.class,
                        source: record.source,
                        stage: stage.clone(),
                        input_file: record.input_file.clone(),
                        output_file: record.output_file.clone(),
                    },
                );
            }
            if interrupted {
                append(
                    &path,
                    &JournalRecord {
                        status,
                        stage: Some(stage),
                        message,
                        updated_at: now(),
                        ..record
                    },
                );
            }
        }
    }
}

impl Inner {
    fn file_lock(&self, file: &str) -> Arc<AsyncMutex<()>> {
        let key = normalize(file);
        let mut locks = self.file_locks.lock().unwrap_or_else(|e| e.into_inner());
        locks
            .entry(key)
            .or_insert_with(|| Arc::new(AsyncMutex::new(())))
            .clone()
    }

    fn register(&self, job_id: &str, meta: Meta) {
        if let Some(path) = &self.journal {
            let record = meta.record(
                job_id,
                JobStatus::Pending,
                "queued",
                Some("Queued".to_string()),
            );
            append(path, &record);
        }
        let mut map = self.meta.lock().unwrap_or_else(|e| e.into_inner());
        map.insert(job_id.to_string(), meta);
    }

    fn set_stage(&self, job_id: &str, stage: &str) {
        let record = {
            let mut map = self.meta.lock().unwrap_or_else(|e| e.into_inner());
            let Some(meta) = map.get_mut(job_id) else {
                return;
            };
            meta.stage = stage.to_string();
            meta.record(job_id, JobStatus::Pending, stage, None)
        };

        emit_event(
            &self.events,
            JobEvent {
                event_type: "job-stage".to_string(),
                job_id: job_id.to_string(),
                status: None,
                progress: None,
                message: None,
                stage: Some(stage.to_string()),
                output_path: None,
                details: None,
            },
        );

        if let Some(path) = &self.journal {
            append(path, &record);
        }
    }

    fn finish(&self, job_id: &str) {
        // A body that returned normally without touching the status has finished, so the job
        // must not stay `pending` and lie about itself.
        let (status, message) = {
            let mut queue = self.queue.lock().unwrap_or_else(|e| e.into_inner());
            let status = match queue.get(job_id).map(|job| job.status.clone()) {
                Some(JobStatus::Pending) | Some(JobStatus::Running) | None => {
                    queue.update_status(job_id, JobStatus::Completed);
                    JobStatus::Completed
                }
                Some(other) => other,
            };
            let message = queue.get(job_id).and_then(|job| job.message.clone());
            (status, message)
        };
        let (stage, record) = {
            let mut map = self.meta.lock().unwrap_or_else(|e| e.into_inner());
            let Some(meta) = map.get_mut(job_id) else {
                return;
            };
            let stage = match status {
                JobStatus::Completed => "done",
                JobStatus::Cancelled => "cancelled",
                JobStatus::Failed => "failed",
                JobStatus::Paused => "paused",
                JobStatus::Pending | JobStatus::Running => "stopped",
            }
            .to_string();
            meta.stage = stage.clone();
            (
                stage.clone(),
                meta.record(job_id, status.clone(), &stage, message),
            )
        };
        let _ = stage;
        if let Some(path) = &self.journal {
            append(path, &record);
        }
    }
}

fn append(path: &Path, record: &JournalRecord) {
    let Ok(line) = serde_json::to_string(record) else {
        return;
    };
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut file| writeln!(file, "{line}"));
}

/// Last record per job id, in the order the jobs were first seen.
fn read_journal(path: &Path) -> Vec<JournalRecord> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut order: Vec<String> = Vec::new();
    let mut latest: HashMap<String, JournalRecord> = HashMap::new();
    for line in text.lines() {
        let Ok(record) = serde_json::from_str::<JournalRecord>(line) else {
            continue; // a torn last line from a crash must not hide the rest
        };
        if !latest.contains_key(&record.id) {
            order.push(record.id.clone());
        }
        latest.insert(record.id.clone(), record);
    }
    order
        .into_iter()
        .filter_map(|id| latest.remove(&id))
        .collect()
}

fn normalize(file: &str) -> String {
    if file.is_empty() {
        String::new()
    } else {
        std::fs::canonicalize(file)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| file.to_string())
    }
}

fn env_limit(name: &str, fallback: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(fallback)
}

/// `<config dir>/server`, so a container can mount one volume for config and journal.
pub fn default_state_dir() -> Option<PathBuf> {
    crate::config::AppConfig::config_dir()
        .ok()
        .map(|dir| dir.join("server"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::{create_event_channel, JobQueue};
    use std::sync::atomic::AtomicUsize;
    use std::time::Duration;

    /// Temp dir without an extra crate; removed on drop.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("subforge_sched_{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn scheduler() -> (Scheduler, TempDir) {
        let dir = TempDir::new();
        let queue: SharedJobQueue = Arc::new(Mutex::new(JobQueue::new()));
        let (events, _rx) = create_event_channel();
        (
            Scheduler::with_journal(queue, events, Some(dir.path().to_path_buf())),
            dir,
        )
    }

    fn add_job(scheduler: &Scheduler, id: &str, kind: JobType) {
        let mut queue = scheduler.inner.queue.lock().unwrap();
        queue.add(super::super::Job {
            id: id.to_string(),
            job_type: kind,
            status: JobStatus::Pending,
            input_file: format!("/tmp/{id}.mkv"),
            output_file: None,
            progress: 0.0,
            message: Some("Queued".to_string()),
        });
    }

    /// Waits until `counter` reaches `target` — no fixed sleeps that a slow runner can miss.
    async fn wait_until(counter: &Arc<AtomicUsize>, target: usize, what: &str) {
        for _ in 0..1000 {
            if counter.load(Ordering::SeqCst) >= target {
                return;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        panic!("timed out waiting for {what}");
    }

    #[test]
    fn classes_split_heavy_from_light() {
        assert_eq!(class_of(&JobType::Transcribe), JobClass::Heavy);
        assert_eq!(class_of(&JobType::Translate), JobClass::Heavy);
        assert_eq!(class_of(&JobType::BatchOperation), JobClass::Heavy);
        assert_eq!(class_of(&JobType::MergeSubtitle), JobClass::Light);
        assert_eq!(class_of(&JobType::ConvertSubtitle), JobClass::Light);
    }

    #[tokio::test]
    async fn heavy_jobs_never_overlap() {
        let (scheduler, _dir) = scheduler();
        let running = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let finished = Arc::new(AtomicUsize::new(0));

        for index in 0..4 {
            let id = format!("h{index}");
            add_job(&scheduler, &id, JobType::Transcribe);
            let running = running.clone();
            let peak = peak.clone();
            let finished = finished.clone();
            scheduler.gated(
                id,
                JobType::Transcribe,
                JobSource::User,
                format!("/tmp/ep{index}.mkv"),
                async move {
                    let now = running.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(now, Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_millis(20)).await;
                    running.fetch_sub(1, Ordering::SeqCst);
                    finished.fetch_add(1, Ordering::SeqCst);
                },
            );
        }

        wait_until(&finished, 4, "four heavy jobs").await;
        assert_eq!(peak.load(Ordering::SeqCst), 1, "two heavy jobs ran at once");
    }

    #[tokio::test]
    async fn light_jobs_run_past_a_heavy_one() {
        let (scheduler, _dir) = scheduler();
        let heavy_started = Arc::new(AtomicUsize::new(0));
        let heavy_release = Arc::new(tokio::sync::Notify::new());

        add_job(&scheduler, "heavy", JobType::Translate);
        let started = heavy_started.clone();
        let release = heavy_release.clone();
        scheduler.gated(
            "heavy".to_string(),
            JobType::Translate,
            JobSource::User,
            "/tmp/a.mkv".to_string(),
            async move {
                started.store(1, Ordering::SeqCst);
                release.notified().await;
            },
        );
        wait_until(&heavy_started, 1, "the heavy job to start").await;

        let light_done = Arc::new(AtomicUsize::new(0));
        add_job(&scheduler, "light", JobType::MergeSubtitle);
        let done = light_done.clone();
        scheduler.gated(
            "light".to_string(),
            JobType::MergeSubtitle,
            JobSource::User,
            "/tmp/b.mkv".to_string(),
            async move {
                done.store(1, Ordering::SeqCst);
            },
        );

        // The heavy job is deliberately still holding its slot.
        wait_until(&light_done, 1, "the light job to finish past a heavy one").await;
        heavy_release.notify_one();
    }

    #[tokio::test]
    async fn a_user_job_overtakes_queued_automation() {
        let (scheduler, _dir) = scheduler();
        let order = Arc::new(Mutex::new(Vec::<String>::new()));
        let blocker_started = Arc::new(AtomicUsize::new(0));
        let blocker_release = Arc::new(tokio::sync::Notify::new());

        add_job(&scheduler, "blocker", JobType::Transcribe);
        let started = blocker_started.clone();
        let release = blocker_release.clone();
        scheduler.gated(
            "blocker".to_string(),
            JobType::Transcribe,
            JobSource::User,
            "/tmp/block.mkv".to_string(),
            async move {
                started.store(1, Ordering::SeqCst);
                release.notified().await;
            },
        );
        wait_until(&blocker_started, 1, "the blocking job to start").await;

        add_job(&scheduler, "auto", JobType::Transcribe);
        let auto_order = order.clone();
        scheduler.gated(
            "auto".to_string(),
            JobType::Transcribe,
            JobSource::Automation,
            "/tmp/auto.mkv".to_string(),
            async move {
                auto_order.lock().unwrap().push("automation".to_string());
            },
        );

        add_job(&scheduler, "user", JobType::Transcribe);
        let user_order = order.clone();
        scheduler.gated(
            "user".to_string(),
            JobType::Transcribe,
            JobSource::User,
            "/tmp/user.mkv".to_string(),
            async move {
                user_order.lock().unwrap().push("user".to_string());
            },
        );

        // Both are queued; only now does the slot free up.
        tokio::time::sleep(Duration::from_millis(50)).await;
        blocker_release.notify_one();

        for _ in 0..1000 {
            if order.lock().unwrap().len() == 2 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert_eq!(*order.lock().unwrap(), vec!["user", "automation"]);
    }

    #[tokio::test]
    async fn two_jobs_on_one_file_never_overlap() {
        let (scheduler, _dir) = scheduler();
        let running = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let finished = Arc::new(AtomicUsize::new(0));

        for id in ["one", "two"] {
            add_job(&scheduler, id, JobType::MergeSubtitle);
            let running = running.clone();
            let peak = peak.clone();
            let finished = finished.clone();
            scheduler.gated(
                id.to_string(),
                JobType::MergeSubtitle,
                JobSource::User,
                "/tmp/same-episode.mkv".to_string(),
                async move {
                    let now = running.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(now, Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_millis(20)).await;
                    running.fetch_sub(1, Ordering::SeqCst);
                    finished.fetch_add(1, Ordering::SeqCst);
                },
            );
        }

        wait_until(&finished, 2, "both jobs on one file").await;
        assert_eq!(
            peak.load(Ordering::SeqCst),
            1,
            "both jobs touched the same file"
        );
    }

    #[tokio::test]
    async fn a_restart_marks_interrupted_jobs_instead_of_forgetting_them() {
        let dir = TempDir::new();
        let path = dir.path().join("jobs.jsonl");
        append(
            &path,
            &JournalRecord {
                id: "lost".to_string(),
                job_type: JobType::Translate,
                status: JobStatus::Running,
                input_file: "/tmp/lost.mkv".to_string(),
                output_file: None,
                class: JobClass::Heavy,
                source: JobSource::Automation,
                stage: Some("running".to_string()),
                message: None,
                updated_at: 1,
            },
        );

        let queue: SharedJobQueue = Arc::new(Mutex::new(JobQueue::new()));
        let (events, _rx) = create_event_channel();
        let scheduler = Scheduler::with_journal(queue, events, Some(dir.path().to_path_buf()));

        let views = scheduler.snapshot();
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].job.id, "lost");
        assert_eq!(views[0].job.status, JobStatus::Failed);
        assert_eq!(views[0].stage.as_deref(), Some("interrupted"));
        assert!(views[0]
            .job
            .message
            .as_deref()
            .unwrap_or_default()
            .contains("restarted"));
    }

    #[tokio::test]
    async fn a_finished_job_reports_done_and_frees_its_slot() {
        let (scheduler, _dir) = scheduler();
        let finished = Arc::new(AtomicUsize::new(0));
        add_job(&scheduler, "staged", JobType::ConvertSubtitle);
        let done = finished.clone();
        scheduler.gated(
            "staged".to_string(),
            JobType::ConvertSubtitle,
            JobSource::User,
            "/tmp/staged.srt".to_string(),
            async move {
                done.store(1, Ordering::SeqCst);
            },
        );

        wait_until(&finished, 1, "the job body").await;
        for _ in 0..1000 {
            if scheduler.stage_of("staged").as_deref() == Some("done") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert_eq!(scheduler.stage_of("staged").as_deref(), Some("done"));
        assert_eq!(scheduler.inner.heavy.running(), 0);
        assert_eq!(scheduler.inner.light.running(), 0);
    }
}
