use std::borrow::Cow;
use std::io::IsTerminal;
use std::time::Duration;

use indicatif::{ProgressBar, ProgressStyle};

#[derive(Clone)]
pub struct Progress {
    bar: ProgressBar,
}

impl Progress {
    pub fn new() -> Self {
        let bar = if std::io::stderr().is_terminal() {
            let pb = ProgressBar::new_spinner();
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
