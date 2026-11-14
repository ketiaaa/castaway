use castaway_lib::supervisor::{Status, Supervisor, SupervisorConfig};
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread::sleep;
use std::time::{Duration, Instant};

const FAKE: &str = env!("CARGO_BIN_EXE_fake_uxplay");
const WAIT: Duration = Duration::from_secs(5);

fn wait_until(timeout: Duration, mut f: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + timeout;
    loop {
        if f() {
            return true;
        }
        if Instant::now() >= end {
            return false;
        }
        sleep(Duration::from_millis(10));
    }
}

struct Env {
    dir: PathBuf,
    history: Arc<Mutex<Vec<Status>>>,
}

impl Env {
    fn new(name: &str) -> Env {
        let dir = std::env::temp_dir().join(format!("castaway-sup-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Env { dir, history: Arc::default() }
    }

    fn sup(&self, cfg: SupervisorConfig) -> Supervisor {
        let h = self.history.clone();
        Supervisor::new(
            PathBuf::from(FAKE),
            cfg,
            Arc::new(move |s| h.lock().unwrap().push(s)),
        )
    }

    fn hist(&self) -> Vec<Status> {
        self.history.lock().unwrap().clone()
    }

    fn path(&self, f: &str) -> PathBuf {
        self.dir.join(f)
    }

    fn pid_arg(&self, f: &str) -> String {
        format!("--pid-file={}", self.path(f).display())
    }

    fn count_arg(&self) -> String {
        format!("--count-file={}", self.path("count").display())
    }

    fn launches(&self) -> usize {
        std::fs::read_to_string(self.path("count")).map(|s| s.lines().count()).unwrap_or(0)
    }

    fn pid(&self, f: &str) -> u32 {
        let p = self.path(f);
        assert!(
            wait_until(WAIT, || std::fs::read_to_string(&p).is_ok_and(|s| s.trim().parse::<u32>().is_ok())),
            "pid file never appeared"
        );
        std::fs::read_to_string(&p).unwrap().trim().parse().unwrap()
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn alive(pid: u32) -> bool {
    let out = Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).contains(&format!("\"{pid}\""))
}

fn quick_cfg() -> SupervisorConfig {
    SupervisorConfig {
        backoff: vec![Duration::from_millis(50)],
        max_failures: 3,
        disconnect_grace: Duration::from_millis(700),
        stable_after: Duration::from_secs(60),
    }
}

fn args(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

fn connected() -> Status {
    Status::Connected { device: "Test iPhone".into() }
}

#[test]
fn default_config_values() {
    let c = SupervisorConfig::default();
    assert_eq!(c.backoff, vec![Duration::from_secs(1), Duration::from_secs(2), Duration::from_secs(4)]);
    assert_eq!(c.max_failures, 5);
    assert_eq!(c.disconnect_grace, Duration::from_secs(2));
    assert_eq!(c.stable_after, Duration::from_secs(10));
}

#[test]
fn supervisor_is_send_and_sync() {
    fn check<T: Send + Sync>() {}
    check::<Supervisor>();
}

#[test]
fn start_reaches_waiting() {
    let env = Env::new("start");
    let sup = env.sup(quick_cfg());
    assert_eq!(sup.status(), Status::Stopped);
    sup.start(args(&["ok"]));
    assert!(wait_until(WAIT, || sup.status() == Status::Waiting));
    assert_eq!(env.hist(), vec![Status::Starting, Status::Waiting]);
    sup.stop();
}

#[test]
fn connect_and_disconnect_lines_update_status() {
    let env = Env::new("connect");
    let sup = env.sup(quick_cfg());
    sup.start(args(&["connect_then_disconnect"]));
    assert!(wait_until(WAIT, || sup.status() == connected()));
    // Closes arrive ~200ms later; inside the 700ms grace we stay Connected.
    sleep(Duration::from_millis(350));
    assert_eq!(sup.status(), connected());
    // After the grace with no new connect we fall back to Waiting.
    assert!(wait_until(WAIT, || sup.status() == Status::Waiting));
    assert_eq!(
        env.hist(),
        vec![Status::Starting, Status::Waiting, connected(), Status::Waiting]
    );
    sup.stop();
}

#[test]
fn connect_inside_grace_keeps_connected() {
    let env = Env::new("reconnect");
    let mut cfg = quick_cfg();
    cfg.disconnect_grace = Duration::from_millis(1500);
    let sup = env.sup(cfg);
    sup.start(args(&["reconnect"]));
    assert!(wait_until(WAIT, || sup.status() == connected()));
    // Closes at +200ms, reconnect at +500ms, grace would have expired at +1700ms.
    sleep(Duration::from_millis(2200));
    assert_eq!(sup.status(), connected());
    assert_eq!(env.hist(), vec![Status::Starting, Status::Waiting, connected()]);
    sup.stop();
}

#[test]
fn crash_restarts_with_backoff() {
    let env = Env::new("crash_restart");
    let mut cfg = quick_cfg();
    cfg.max_failures = 100;
    let sup = env.sup(cfg);
    sup.start(args(&["crash", &env.count_arg()]));
    assert!(wait_until(WAIT, || env.launches() >= 3));
    sup.stop();
    assert_eq!(sup.status(), Status::Stopped);
}

#[test]
fn repeated_crash_ends_in_error_with_last_stderr_line() {
    let env = Env::new("crash_final");
    let sup = env.sup(quick_cfg());
    sup.start(args(&["crash", &env.count_arg()]));
    assert!(wait_until(WAIT, || matches!(sup.status(), Status::Error(m) if m.starts_with("UxPlay keeps stopping"))));
    match sup.status() {
        Status::Error(m) => assert!(m.contains("fake failure"), "{m}"),
        s => panic!("{s:?}"),
    }
    assert_eq!(env.launches(), 3);
    sleep(Duration::from_millis(400));
    assert_eq!(env.launches(), 3, "no launches after max_failures");
    sup.stop();
}

#[test]
fn stable_run_resets_failure_counter() {
    let env = Env::new("stable");
    let mut cfg = quick_cfg();
    cfg.max_failures = 2;
    cfg.stable_after = Duration::from_millis(100); // child lives 300ms
    let sup = env.sup(cfg);
    sup.start(args(&["crash_after_ok", &env.count_arg()]));
    assert!(wait_until(WAIT, || env.launches() >= 4));
    assert!(!matches!(sup.status(), Status::Error(_)), "{:?}", sup.status());
    sup.stop();
}

#[test]
fn stop_kills_child_and_reports_stopped() {
    let env = Env::new("stop");
    let sup = env.sup(quick_cfg());
    sup.start(args(&["ok", &env.pid_arg("pid"), &env.count_arg()]));
    let pid = env.pid("pid");
    assert!(alive(pid));
    sup.stop();
    assert_eq!(sup.status(), Status::Stopped);
    assert!(wait_until(WAIT, || !alive(pid)), "child still alive");
    sleep(Duration::from_millis(300));
    assert_eq!(env.launches(), 1, "stop must not trigger a restart");
    assert_eq!(sup.status(), Status::Stopped);
}

#[test]
fn dropping_supervisor_kills_child() {
    let env = Env::new("drop");
    let sup = env.sup(quick_cfg());
    sup.start(args(&["ok", &env.pid_arg("pid")]));
    let pid = env.pid("pid");
    assert!(alive(pid));
    drop(sup);
    assert!(wait_until(WAIT, || !alive(pid)), "child survived drop");
}

#[test]
fn error_event_from_live_process_sets_error_status_and_keeps_running() {
    let env = Env::new("error_live");
    let sup = env.sup(quick_cfg());
    sup.start(args(&["error_live", &env.pid_arg("pid"), &env.count_arg()]));
    assert!(wait_until(WAIT, || matches!(sup.status(), Status::Error(m) if m.contains("10013"))));
    let pid = env.pid("pid");
    sleep(Duration::from_millis(400));
    assert!(alive(pid));
    assert_eq!(env.launches(), 1);
    assert!(matches!(sup.status(), Status::Error(_)));
    sup.stop();
    assert!(wait_until(WAIT, || !alive(pid)));
}

#[test]
fn restart_with_new_args_replaces_child() {
    let env = Env::new("restart");
    let sup = env.sup(quick_cfg());
    sup.start(args(&["ok", &env.pid_arg("a")]));
    let a = env.pid("a");
    assert!(wait_until(WAIT, || sup.status() == Status::Waiting));
    sup.restart(args(&["connect", &env.pid_arg("b")]));
    let b = env.pid("b");
    assert_ne!(a, b);
    assert!(wait_until(WAIT, || !alive(a)), "old child still alive");
    assert!(alive(b));
    assert!(wait_until(WAIT, || sup.status() == connected()));
    sup.stop();
    assert!(wait_until(WAIT, || !alive(b)));
}

