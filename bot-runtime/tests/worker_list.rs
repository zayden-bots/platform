use bot_runtime::BotError;
use bot_runtime::worker::WorkerList;
use zayden_core::WorkerJob;

#[test]
fn worker_list_rejects_duplicate_job_ids() {
    let mut list = WorkerList::default();
    list.job(WorkerJob::new("twice", "0 0 * * * * *"));
    list.job(WorkerJob::new("twice", "0 0 * * * * *"));

    assert!(matches!(
        list.finish(),
        Err(BotError::Other(msg)) if msg == "duplicate worker job twice"
    ));
}
