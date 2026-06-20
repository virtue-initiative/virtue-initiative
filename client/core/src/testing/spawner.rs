use crate::events::bus::Spawner;

/// Test [`Spawner`] that runs each job immediately, inline, on the calling
/// thread. Because the job (and any events it emits) execute before `spawn`
/// returns, an `emitter.spawn(...)` from within a handler cascades its follow-up
/// events into the **same** `iter()` — keeping [`EventTester`](super::EventTester)
/// deterministic with no threads or timing involved.
pub struct InlineSpawner;

impl Spawner for InlineSpawner {
    fn spawn(&self, job: Box<dyn FnOnce() + Send + 'static>) {
        job();
    }
}
