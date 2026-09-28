//! Native callbacks wake all active UI listeners. Each listener registers
//! before checking its queues, so events cannot fall between a check and wait.
use std::time::Duration;

use event_listener::{Event, EventListener};
use gpui_kit::BackgroundExecutor;

static CHANGED: Event = Event::new();

pub(crate) fn notify() {
    CHANGED.notify(usize::MAX);
}

pub(crate) fn listen() -> EventListener {
    CHANGED.listen()
}

pub(crate) async fn wait(
    listener: EventListener,
    deadline: Option<Duration>,
    executor: &BackgroundExecutor,
) {
    if let Some(delay) = deadline {
        futures::future::select(Box::pin(listener), Box::pin(executor.timer(delay))).await;
    } else {
        listener.await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::FutureExt;

    #[test]
    fn notifications_wake_every_registered_listener_without_polling() {
        let first = listen();
        let second = listen();
        notify();
        assert!(first.now_or_never().is_some());
        assert!(second.now_or_never().is_some());
        assert!(listen().now_or_never().is_none());
    }
}
