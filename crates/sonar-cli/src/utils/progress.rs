use std::borrow::Cow;
use std::io::{self, IsTerminal, Write};
use std::sync::LazyLock;
use std::time::Duration;

use indicatif::{MultiProgress, ProgressBar, ProgressStyle};

// Progress bars and diagnostics share one draw lock so a log record cannot
// appear in the middle of a spinner line.
static PROGRESS: LazyLock<MultiProgress> = LazyLock::new(MultiProgress::new);

pub struct ProgressWriter;

impl Write for ProgressWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        PROGRESS.suspend(|| {
            let mut stderr = io::stderr().lock();
            stderr.write_all(buf)?;
            stderr.flush()?;
            Ok(buf.len())
        })
    }

    fn flush(&mut self) -> io::Result<()> {
        io::stderr().flush()
    }
}

#[derive(Clone)]
pub struct Progress {
    bar: ProgressBar,
}

impl Progress {
    pub fn new() -> Self {
        let bar = if std::io::stderr().is_terminal() {
            let pb = PROGRESS.add(ProgressBar::new_spinner());
            let style = ProgressStyle::with_template("  {spinner:.cyan} {msg}")
                .expect("progress spinner template is valid")
                .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏");
            pb.set_style(style);
            pb.enable_steady_tick(Duration::from_millis(80));
            pb
        } else {
            ProgressBar::hidden()
        };

        Self { bar }
    }

    pub fn set_message(&self, msg: impl Into<Cow<'static, str>>) {
        self.bar.set_message(msg.into());
        // Force an immediate redraw so short-lived stages are still visible.
        self.bar.tick();
    }

    pub fn finish(&self) {
        self.bar.finish_and_clear();
    }
}
