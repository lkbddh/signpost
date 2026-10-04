//! What the unit tests share.

pub mod frosted;
pub mod headless;
pub mod logs;
pub mod peer;

/// A pipe at `path` that nothing writes to: a plain open or read of it never returns.
pub fn pipe_at(path: &std::path::Path) {
    rustix::fs::mkfifoat(
        rustix::fs::CWD,
        path,
        rustix::fs::Mode::from_raw_mode(0o600),
    )
    .expect("a pipe");
}

/// What `work` gives, when it finishes within five seconds on a thread of its own; `None` when it hangs.
pub fn finishes<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    let (sender, answer) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(work());
    });
    answer.recv_timeout(std::time::Duration::from_secs(5)).ok()
}
