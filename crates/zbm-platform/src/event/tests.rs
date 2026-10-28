use zbm_testkit::TempDir;

use super::*;
use crate::KernelLog;
use crate::files::RealFiles;

#[test]
fn workers_log_to_the_log_of_the_thread_that_started_them() {
    let dir = TempDir::new("zbm-platform-event");
    let kmsg = dir.join("kmsg");
    let log = KernelLog::new(&RealFiles, "7", dir.path(), 1)
        .with_device(&kmsg)
        .install();
    let span = tracing::info_span!("unlocking", volume = "cryptroot").entered();
    let worker = spawn_traced("worker", || {
        zbm_trace::warn!("from the worker");
        Span::current().metadata().map(tracing::Metadata::name)
    })
    .unwrap();
    assert_eq!(worker.join().unwrap(), Some("unlocking"));
    let scoped = std::thread::scope(|scope| {
        spawn_scoped_traced(scope, || {
            zbm_trace::warn!("from a scoped worker");
            7
        })
        .join()
        .unwrap()
    });
    assert_eq!(scoped, 7);
    drop(span);
    drop(log);
    #[allow(
        clippy::disallowed_methods,
        reason = "the test reads the real file it logged to"
    )]
    let kmsg = std::fs::read_to_string(&kmsg).unwrap_or_default();
    assert!(kmsg.contains("from the worker"), "{kmsg}");
    assert!(kmsg.contains("from a scoped worker"), "{kmsg}");
}

#[test]
fn a_worker_runs_outside_any_span_of_a_thread_without_one() {
    let worker = spawn_traced("quiet", || {
        zbm_trace::warn!("to no log");
        Span::current().is_none()
    })
    .unwrap();
    assert!(worker.join().unwrap());
}
