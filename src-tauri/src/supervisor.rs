//! Runs UxPlay as a supervised child process: status tracking, crash restart
//! with backoff, and kill-on-close via a Windows Job Object.

use crate::log_parser::{UxEvent, parse_line};
use serde::Serialize;
use std::io::{BufRead, BufReader, Read};
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use win32job::Job;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const POLL: Duration = Duration::from_millis(10);

#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum Status {
    Stopped,
    Starting,
    Waiting,
    Connected { device: String },
    Error(String),
}

#[derive(Debug, Clone)]
pub struct SupervisorConfig {
    pub backoff: Vec<Duration>,
    pub max_failures: usize,
    pub disconnect_grace: Duration,
    pub stable_after: Duration,
}

impl Default for SupervisorConfig {
    fn default() -> Self {
        Self {
            backoff: vec![
                Duration::from_secs(1),
                Duration::from_secs(2),
                Duration::from_secs(4),
            ],
            max_failures: 5,
            disconnect_grace: Duration::from_secs(2),
            stable_after: Duration::from_secs(10),
        }
    }
}

type OnStatus = Arc<dyn Fn(Status) + Send + Sync>;

/// Lock a mutex, recovering from poisoning instead of panicking.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Shared status cell. Changes are recorded under one lock and delivered, in
/// order, by a dedicated dispatcher thread, so `on_status` never runs while a
/// Supervisor lock is held (it may call back into the Supervisor).
struct StatusCell {
    state: Mutex<CellState>,
}

struct CellState {
    current: Status,
    tx: Option<mpsc::Sender<Status>>,
}

impl StatusCell {
    /// Set `status`, unless `stop` is raised (a stale session must not report).
    fn set(&self, status: Status, stop: Option<&AtomicBool>) {
        let mut st = lock(&self.state);
        if stop.is_some_and(|s| s.load(Ordering::SeqCst)) || st.current == status {
            return;
        }
        st.current = status.clone();
        if let Some(tx) = &st.tx {
            let _ = tx.send(status);
        }
    }

    fn get(&self) -> Status {
        lock(&self.state).current.clone()
    }

    fn close(&self) {
        lock(&self.state).tx = None;
    }
}

struct Session {
    stop: Arc<AtomicBool>,
    handle: JoinHandle<()>,
}

pub struct Supervisor {
    program: PathBuf,
    cfg: SupervisorConfig,
    job: Option<Arc<Job>>,
    cell: Arc<StatusCell>,
    /// Control lock: held across all of start/stop/restart so they are atomic
    /// with respect to each other.
    session: Mutex<Option<Session>>,
    dispatcher: Mutex<Option<JoinHandle<()>>>,
}

struct Line {
    stderr: bool,
    text: String,
    event: Option<UxEvent>,
}

/// A job object that kills every assigned process when its last handle closes
/// (including when this process dies).
fn make_job() -> Option<Arc<Job>> {
    let job = Job::create().ok()?;
    let mut info = job.query_extended_limit_info().ok()?;
    info.limit_kill_on_job_close();
    job.set_extended_limit_info(&mut info).ok()?;
    Some(Arc::new(job))
}

fn spawn_reader<R: Read + Send + 'static>(
    r: R,
    stderr: bool,
    tx: mpsc::Sender<Line>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        for raw in BufReader::new(r).split(b'\n') {
            let Ok(raw) = raw else { break };
            let text = String::from_utf8_lossy(&raw)
                .trim_end_matches('\r')
                .to_string();
            let event = parse_line(&text);
            if tx.send(Line { stderr, text, event }).is_err() {
                break;
            }
        }
    })
}

fn kill(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// Per-launch bookkeeping fed by the reader threads.
#[derive(Default)]
struct LaunchState {
    last_err: String,
    last_out: String,
    disc_deadline: Option<Instant>,
}

impl LaunchState {
    fn last_line(&self) -> String {
        if self.last_err.is_empty() {
            self.last_out.clone()
        } else {
            self.last_err.clone()
        }
    }
}

/// Everything one session thread needs.
struct Runner {
    program: PathBuf,
    args: Vec<String>,
    cfg: SupervisorConfig,
    job: Arc<Job>,
    cell: Arc<StatusCell>,
    stop: Arc<AtomicBool>,
}

impl Runner {
    fn set(&self, s: Status) {
        self.cell.set(s, Some(&self.stop));
    }

    fn stopped(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }

    /// Sleep up to `d`; returns true as soon as stop is raised.
    fn sleep_or_stop(&self, d: Duration) -> bool {
        let end = Instant::now() + d;
        while Instant::now() < end && !self.stopped() {
            thread::sleep(POLL.min(end.saturating_duration_since(Instant::now())));
        }
        self.stopped()
    }

    fn spawn(&self) -> Result<Child, String> {
        let mut child = Command::new(&self.program)
            .args(&self.args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| format!("could not start {}: {e}", self.program.display()))?;
        if let Err(e) = self.job.assign_process(child.as_raw_handle() as isize) {
            kill(&mut child);
            return Err(format!("could not assign process to job object: {e}"));
        }
        Ok(child)
    }

    fn on_line(&self, l: Line, st: &mut LaunchState) {
        let t = l.text.trim();
        if !t.is_empty() {
            let dst = if l.stderr { &mut st.last_err } else { &mut st.last_out };
            *dst = t.to_string();
        }
        match l.event {
            Some(UxEvent::Listening) => {
                st.disc_deadline = None;
                self.set(Status::Waiting);
            }
            Some(UxEvent::ClientConnected { device }) => {
                st.disc_deadline = None;
                self.set(Status::Connected {
                    device: device.unwrap_or_else(|| "Unknown device".to_string()),
                });
            }
            Some(UxEvent::ClientDisconnected) => {
                if matches!(self.cell.get(), Status::Connected { .. }) {
                    st.disc_deadline = Some(Instant::now() + self.cfg.disconnect_grace);
                }
            }
            Some(UxEvent::Error(msg)) => {
                st.disc_deadline = None;
                self.set(Status::Error(msg));
            }
            None => {}
        }
    }

    /// Run one child until it exits or stop is raised. Returns the last output
    /// line when the child died on its own, `None` when we stopped it.
    fn run_once(&self) -> Option<String> {
        let mut child = match self.spawn() {
            Ok(c) => c,
            Err(e) => return Some(e),
        };
        let (tx, rx): (_, Receiver<Line>) = mpsc::channel();
        let mut readers = Vec::new();
        if let Some(o) = child.stdout.take() {
            readers.push(spawn_reader(o, false, tx.clone()));
        }
        if let Some(e) = child.stderr.take() {
            readers.push(spawn_reader(e, true, tx.clone()));
        }
        drop(tx);

        let mut st = LaunchState::default();
        let mut exit_note = None;
        loop {
            if self.stopped() {
                kill(&mut child);
                break;
            }
            if let Ok(l) = rx.recv_timeout(POLL) {
                self.on_line(l, &mut st);
            }
            if st.disc_deadline.is_some_and(|d| Instant::now() >= d) {
                st.disc_deadline = None;
                if matches!(self.cell.get(), Status::Connected { .. }) {
                    self.set(Status::Waiting);
                }
            }
            match child.try_wait() {
                Ok(Some(code)) => {
                    exit_note = Some(format!("process exited ({code})"));
                    break;
                }
                Ok(None) => {}
                Err(e) => {
                    kill(&mut child);
                    exit_note = Some(format!("lost track of process: {e}"));
                    break;
                }
            }
        }
        // Drain what the child printed before exiting (bounded).
        let drain_end = Instant::now() + Duration::from_secs(1);
        while let Some(left) = drain_end.checked_duration_since(Instant::now()) {
            match rx.recv_timeout(left) {
                Ok(l) => self.on_line(l, &mut st),
                Err(_) => break,
            }
        }
        for r in readers {
            let _ = r.join();
        }
        if self.stopped() {
            return None;
        }
        let last = st.last_line();
        Some(if last.is_empty() {
            exit_note.unwrap_or_default()
        } else {
            last
        })
    }

    fn run(&self) {
        self.set(Status::Starting);
        let mut failures = 0usize;
        loop {
            let started = Instant::now();
            let Some(last_line) = self.run_once() else {
                return;
            };
            if started.elapsed() >= self.cfg.stable_after {
                failures = 0;
            }
            failures += 1;
            if failures >= self.cfg.max_failures {
                self.set(Status::Error(format!("UxPlay keeps stopping: {last_line}")));
                return;
            }
            self.set(Status::Starting);
            let idx = (failures - 1).min(self.cfg.backoff.len().saturating_sub(1));
            let wait = self.cfg.backoff.get(idx).copied().unwrap_or_default();
            if self.sleep_or_stop(wait) {
                return;
            }
        }
    }
}

impl Supervisor {
    pub fn new(program: PathBuf, cfg: SupervisorConfig, on_status: OnStatus) -> Supervisor {
        let (tx, rx) = mpsc::channel::<Status>();
        let spawned = thread::Builder::new()
            .name("uxplay-status".into())
            .spawn(move || {
                for s in rx {
                    on_status(s);
                }
            });
        let (tx, dispatcher) = match spawned {
            Ok(h) => (Some(tx), Some(h)),
            Err(_) => (None, None),
        };
        Supervisor {
            program,
            cfg,
            job: make_job(),
            cell: Arc::new(StatusCell {
                state: Mutex::new(CellState {
                    current: Status::Stopped,
                    tx,
                }),
            }),
            session: Mutex::new(None),
            dispatcher: Mutex::new(dispatcher),
        }
    }

    pub fn status(&self) -> Status {
        self.cell.get()
    }

    /// Start UxPlay with `args`. No-op while a session is still running.
    pub fn start(&self, args: Vec<String>) {
        let mut session = lock(&self.session);
        self.start_locked(&mut session, args);
    }

    fn start_locked(&self, session: &mut Option<Session>, args: Vec<String>) {
        if session.as_ref().is_some_and(|s| !s.handle.is_finished()) {
            return;
        }
        if let Some(old) = session.take() {
            let _ = old.handle.join();
        }
        let Some(job) = self.job.clone() else {
            self.cell.set(
                Status::Error("could not create job object".to_string()),
                None,
            );
            return;
        };
        let stop = Arc::new(AtomicBool::new(false));
        let runner = Runner {
            program: self.program.clone(),
            args,
            cfg: self.cfg.clone(),
            job,
            cell: self.cell.clone(),
            stop: stop.clone(),
        };
        self.cell.set(Status::Starting, None);
        let spawned = thread::Builder::new()
            .name("uxplay-supervisor".into())
            .spawn(move || runner.run());
        match spawned {
            Ok(handle) => *session = Some(Session { stop, handle }),
            Err(e) => self.cell.set(
                Status::Error(format!("could not start supervisor thread: {e}")),
                None,
            ),
        }
    }

    /// Stop the session thread (killing the child) and wait for it.
    fn halt_locked(session: &mut Option<Session>) {
        if let Some(s) = session.take() {
            s.stop.store(true, Ordering::SeqCst);
            let _ = s.handle.join();
        }
    }

    pub fn stop(&self) {
        let mut session = lock(&self.session);
        Self::halt_locked(&mut session);
        self.cell.set(Status::Stopped, None);
    }

    pub fn restart(&self, args: Vec<String>) {
        let mut session = lock(&self.session);
        Self::halt_locked(&mut session);
        self.cell.set(Status::Stopped, None);
        self.start_locked(&mut session, args);
    }
}

impl Drop for Supervisor {
    fn drop(&mut self) {
        Self::halt_locked(&mut lock(&self.session));
        self.cell.close();
        if let Some(h) = lock(&self.dispatcher).take() {
            // Dropped from inside a callback: cannot join ourselves.
            if h.thread().id() != thread::current().id() {
                let _ = h.join();
            }
        }
    }
}
