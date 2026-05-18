use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Instant;

use crate::repl::logic::core::{ReplCompletion, ReplCompletionContext};

#[derive(Debug, Clone)]
pub struct ReplCompletionRequest {
    pub input: String,
    pub cursor: usize,
    pub generation: u64,
    pub enqueued_at: Instant,
    pub event_received_at: Option<Instant>,
}

#[derive(Debug, Clone)]
pub struct ReplCompletionResult {
    pub input: String,
    pub cursor: usize,
    pub generation: u64,
    pub completion: ReplCompletion,
    pub enqueued_at: Instant,
    pub event_received_at: Option<Instant>,
}

pub trait ReplCompletionProvider {
    fn submit(&mut self, request: ReplCompletionRequest);
    fn poll_ready(&mut self) -> Option<ReplCompletionResult>;
    fn schedule_context_refresh(&mut self, context: ReplCompletionContext);
}

#[derive(Debug, Default, Clone)]
pub struct ReplCompletionController {
    current_generation: u64,
    last_requested: Option<(String, usize)>,
    visible_generation: Option<u64>,
}

impl ReplCompletionController {
    pub fn submit_if_changed(
        &mut self,
        provider: &mut dyn ReplCompletionProvider,
        input: &str,
        cursor: usize,
        event_received_at: Option<Instant>,
    ) -> bool {
        if !ReplCompletionContext::should_request(input, cursor) {
            self.cancel_pending();
            return false;
        }

        let key = (input.to_string(), cursor);
        if self.last_requested.as_ref() == Some(&key) {
            return false;
        }

        self.current_generation = self.current_generation.saturating_add(1);
        self.last_requested = Some(key.clone());
        provider.submit(ReplCompletionRequest {
            input: key.0,
            cursor: key.1,
            generation: self.current_generation,
            enqueued_at: Instant::now(),
            event_received_at,
        });
        true
    }

    pub fn cancel_pending(&mut self) {
        self.current_generation = self.current_generation.saturating_add(1);
        self.last_requested = None;
        self.visible_generation = None;
    }

    pub fn accept_ready(&mut self, result: ReplCompletionResult) -> Option<ReplCompletionResult> {
        if result.generation != self.current_generation {
            return None;
        }
        self.visible_generation = Some(result.generation);
        Some(result)
    }
}

#[derive(Default)]
struct WorkerMailbox {
    pending: Option<ReplCompletionRequest>,
    pending_context: Option<ReplCompletionContext>,
    ready: VecDeque<ReplCompletionResult>,
    closed: bool,
}

pub struct BackgroundReplCompletionProvider {
    mailbox: Arc<(Mutex<WorkerMailbox>, Condvar)>,
    worker: Option<JoinHandle<()>>,
}

impl BackgroundReplCompletionProvider {
    pub fn new(context: ReplCompletionContext) -> Self {
        let mailbox = Arc::new((Mutex::new(WorkerMailbox::default()), Condvar::new()));
        let worker_mailbox = Arc::clone(&mailbox);
        let worker = thread::Builder::new()
            .name("xldr-repl-completion".to_string())
            .spawn(move || worker_loop(worker_mailbox, context))
            .expect("completion worker thread should start");
        Self {
            mailbox,
            worker: Some(worker),
        }
    }
}

impl ReplCompletionProvider for BackgroundReplCompletionProvider {
    fn submit(&mut self, request: ReplCompletionRequest) {
        let (lock, wake) = &*self.mailbox;
        let mut mailbox = lock.lock().expect("completion mailbox lock should work");
        mailbox.pending = Some(request);
        wake.notify_one();
    }

    fn poll_ready(&mut self) -> Option<ReplCompletionResult> {
        let (lock, _) = &*self.mailbox;
        let mut mailbox = lock.lock().expect("completion mailbox lock should work");
        mailbox.ready.pop_front()
    }

    fn schedule_context_refresh(&mut self, context: ReplCompletionContext) {
        let (lock, wake) = &*self.mailbox;
        let mut mailbox = lock.lock().expect("completion mailbox lock should work");
        mailbox.pending_context = Some(context);
        wake.notify_one();
    }
}

impl Drop for BackgroundReplCompletionProvider {
    fn drop(&mut self) {
        let (lock, wake) = &*self.mailbox;
        {
            let mut mailbox = lock.lock().expect("completion mailbox lock should work");
            mailbox.closed = true;
            wake.notify_one();
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn worker_loop(mailbox: Arc<(Mutex<WorkerMailbox>, Condvar)>, mut context: ReplCompletionContext) {
    loop {
        let request = {
            let (lock, wake) = &*mailbox;
            let mut state = lock.lock().expect("completion mailbox lock should work");
            while state.pending.is_none() && state.pending_context.is_none() && !state.closed {
                state = wake
                    .wait(state)
                    .expect("completion worker should wait on mailbox");
            }
            if state.closed {
                return;
            }
            let pending_context = state.pending_context.take();
            let pending_request = state.pending.take();
            (pending_context, pending_request)
        };

        if let Some(next_context) = request.0 {
            context = next_context;
        }

        let Some(request) = request.1 else {
            continue;
        };

        let started = Instant::now();
        let mut completion = context.completions(&request.input, request.cursor);
        completion
            .telemetry
            .record_completion_queue(started.saturating_duration_since(request.enqueued_at));

        let (lock, _) = &*mailbox;
        let mut state = lock.lock().expect("completion mailbox lock should work");
        state.ready.push_back(ReplCompletionResult {
            input: request.input,
            cursor: request.cursor,
            generation: request.generation,
            completion,
            enqueued_at: request.enqueued_at,
            event_received_at: request.event_received_at,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct RecordingProvider {
        requests: Vec<ReplCompletionRequest>,
        ready: VecDeque<ReplCompletionResult>,
    }

    impl ReplCompletionProvider for RecordingProvider {
        fn submit(&mut self, request: ReplCompletionRequest) {
            self.requests.push(request);
        }

        fn poll_ready(&mut self) -> Option<ReplCompletionResult> {
            self.ready.pop_front()
        }

        fn schedule_context_refresh(&mut self, _context: ReplCompletionContext) {}
    }

    #[test]
    fn controller_coalesces_duplicate_input_cursor_submissions() {
        let mut controller = ReplCompletionController::default();
        let mut provider = RecordingProvider::default();

        assert!(controller.submit_if_changed(&mut provider, "Str", 3, None));
        assert!(!controller.submit_if_changed(&mut provider, "Str", 3, None));
        assert_eq!(provider.requests.len(), 1);
        assert_eq!(provider.requests[0].generation, 1);
    }

    #[test]
    fn controller_drops_stale_generations_and_accepts_latest() {
        let mut controller = ReplCompletionController::default();
        let mut provider = RecordingProvider::default();

        assert!(controller.submit_if_changed(&mut provider, "St", 2, None));
        assert!(controller.submit_if_changed(&mut provider, "Str", 3, None));
        assert_eq!(provider.requests.len(), 2);

        let stale = ReplCompletionResult {
            input: "St".to_string(),
            cursor: 2,
            generation: provider.requests[0].generation,
            completion: ReplCompletion::default(),
            enqueued_at: Instant::now(),
            event_received_at: None,
        };
        assert!(controller.accept_ready(stale).is_none());

        let fresh = ReplCompletionResult {
            input: "Str".to_string(),
            cursor: 3,
            generation: provider.requests[1].generation,
            completion: ReplCompletion::default(),
            enqueued_at: Instant::now(),
            event_received_at: None,
        };
        assert!(controller.accept_ready(fresh).is_some());
    }

    #[test]
    fn controller_clears_requests_when_no_completion_should_be_visible() {
        let mut controller = ReplCompletionController::default();
        let mut provider = RecordingProvider::default();

        assert!(controller.submit_if_changed(&mut provider, "Str", 3, None));
        assert!(!controller.submit_if_changed(&mut provider, "", 0, None));
        assert_eq!(provider.requests.len(), 1);
    }

    #[test]
    fn background_provider_applies_scheduled_context_before_next_completion() {
        let initial = ReplCompletionContext::default();
        let mut provider = BackgroundReplCompletionProvider::new(initial);

        let mut refreshed = ReplCompletionContext::default();
        refreshed.insert_callable_signature_for_test(
            "fresh",
            "Fresh::fresh",
            "fresh(value: String) -> Unit",
        );
        provider.schedule_context_refresh(refreshed.clone());

        provider.submit(ReplCompletionRequest {
            input: "fresh(".to_string(),
            cursor: "fresh(".len(),
            generation: 1,
            enqueued_at: Instant::now(),
            event_received_at: None,
        });

        let deadline = Instant::now() + std::time::Duration::from_secs(2);
        loop {
            if let Some(result) = provider.poll_ready() {
                let signature = result
                    .completion
                    .signature
                    .expect("refreshed signature should be visible");
                assert!(
                    signature
                        .lines
                        .iter()
                        .any(|line| line.contains("Fresh::fresh") && line.contains("-> Unit")),
                    "{signature:?}"
                );
                break;
            }
            assert!(Instant::now() < deadline, "completion result should arrive");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
}
