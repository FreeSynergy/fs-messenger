//! Every log line of every crate in this test binary, at every level.

use std::io::Write;
use std::sync::{Arc, Mutex, OnceLock};

use tracing_subscriber::fmt::MakeWriter;

/// All that is logged in this test binary, from every crate, at every level.
#[derive(Clone, Default)]
pub struct Captured(Arc<Mutex<Vec<u8>>>);

impl Captured {
    /// All lines so far.
    pub fn lines(&self) -> Vec<u8> {
        self.0.lock().unwrap().clone()
    }
}

impl Write for Captured {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(data);
        Ok(data.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for Captured {
    type Writer = Self;
    fn make_writer(&'a self) -> Self {
        self.clone()
    }
}

pub fn captured() -> &'static Captured {
    static LOG: OnceLock<Captured> = OnceLock::new();
    LOG.get_or_init(|| {
        let log = Captured::default();
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_ansi(false)
            .with_writer(log.clone())
            .init();
        log
    })
}
