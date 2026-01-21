#![allow(dead_code)]

use std::sync::{Arc, Mutex};
use tracing_subscriber::layer::SubscriberExt;

/// Captures tracing logs during tests for verification.
pub struct TestLogCapture {
    logs: Arc<Mutex<Vec<CapturedLog>>>,
    _guard: tracing::subscriber::DefaultGuard,
}

#[derive(Debug, Clone)]
pub struct CapturedLog {
    pub level: tracing::Level,
    pub target: String,
    pub message: String,
    pub fields: Vec<(String, String)>,
}

impl TestLogCapture {
    /// Start capturing logs. Returns guard that stops capture on drop.
    pub fn start() -> Self {
        let logs = Arc::new(Mutex::new(Vec::new()));
        let layer = CaptureLayer { logs: logs.clone() };

        // We use a new subscriber for each test capture
        // Note: This replaces the global default if one was set,
        // but set_default only affects the current thread if using scoped,
        // OR globally if set_global_default.
        // tracing::subscriber::set_default sets the default for the current thread
        // (if using tracing 0.1 and specific features, actually set_default returns a guard
        // and sets it as the default for the thread).

        let subscriber = tracing_subscriber::registry().with(layer);
        let guard = tracing::subscriber::set_default(subscriber);

        Self {
            logs,
            _guard: guard,
        }
    }

    /// Assert a message was logged containing the given substring.
    pub fn assert_logged(&self, needle: &str) {
        let logs = self.logs.lock().unwrap();
        let found = logs.iter().any(|l| l.message.contains(needle));
        assert!(
            found,
            "Expected log containing '{}'. Logged: {:#?}",
            needle,
            logs.iter().map(|l| &l.message).collect::<Vec<_>>()
        );
    }

    /// Assert a message was logged at the given level.
    pub fn assert_logged_at_level(&self, level: tracing::Level, needle: &str) {
        let logs = self.logs.lock().unwrap();
        let found = logs
            .iter()
            .any(|l| l.level == level && l.message.contains(needle));
        assert!(
            found,
            "Expected {} log containing '{}'. Logged: {:#?}",
            level,
            needle,
            logs.iter()
                .filter(|l| l.level == level)
                .map(|l| &l.message)
                .collect::<Vec<_>>()
        );
    }

    /// Assert no errors were logged.
    pub fn assert_no_errors(&self) {
        let logs = self.logs.lock().unwrap();
        let errors: Vec<_> = logs
            .iter()
            .filter(|l| l.level == tracing::Level::ERROR)
            .collect();
        assert!(errors.is_empty(), "Unexpected errors: {:#?}", errors);
    }

    /// Assert a structured field was logged.
    pub fn assert_field_logged(&self, field_name: &str, field_value: &str) {
        let logs = self.logs.lock().unwrap();
        let found = logs.iter().any(|l| {
            l.fields
                .iter()
                .any(|(k, v)| k == field_name && v.contains(field_value))
        });
        assert!(
            found,
            "Expected field {}={}. Logged fields: {:#?}",
            field_name,
            field_value,
            logs.iter().map(|l| &l.fields).collect::<Vec<_>>()
        );
    }

    /// Get all captured logs.
    pub fn logs(&self) -> Vec<CapturedLog> {
        self.logs.lock().unwrap().clone()
    }
}

// Tracing layer that captures to vec
struct CaptureLayer {
    logs: Arc<Mutex<Vec<CapturedLog>>>,
}

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for CaptureLayer {
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let mut visitor = FieldVisitor::default();
        event.record(&mut visitor);

        let log = CapturedLog {
            level: *event.metadata().level(),
            target: event.metadata().target().to_string(),
            message: visitor.message,
            fields: visitor.fields,
        };

        self.logs.lock().unwrap().push(log);
    }
}

#[derive(Default)]
struct FieldVisitor {
    message: String,
    fields: Vec<(String, String)>,
}

impl tracing::field::Visit for FieldVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        let name = field.name();
        if name == "message" {
            self.message = format!("{:?}", value);
        } else {
            self.fields.push((name.to_string(), format!("{:?}", value)));
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        let name = field.name();
        if name == "message" {
            self.message = value.to_string();
        } else {
            self.fields.push((name.to_string(), value.to_string()));
        }
    }
}

/// Assert log was emitted within test scope
#[macro_export]
macro_rules! assert_logged {
    ($capture:expr, $needle:expr) => {
        $capture.assert_logged($needle)
    };
    ($capture:expr, $level:expr, $needle:expr) => {
        $capture.assert_logged_at_level($level, $needle)
    };
}
