use std::io::{self, Write};
use unicode_width::UnicodeWidthStr;

pub(super) struct TestProgress<W: Write> {
    writer: W,
    enabled: bool,
    visible: bool,
    width: Box<dyn FnMut() -> Option<u16>>,
}

impl<W: Write> TestProgress<W> {
    pub(super) fn new(writer: W, enabled: bool) -> Self {
        Self::with_width_source(writer, enabled, stderr_width)
    }
    fn with_width_source(
        writer: W,
        enabled: bool,
        width: impl FnMut() -> Option<u16> + 'static,
    ) -> Self {
        Self {
            writer,
            enabled,
            visible: false,
            width: Box::new(width),
        }
    }
    pub(super) fn preparing(&mut self) -> io::Result<()> {
        self.show("Preparing", "standard environment")
    }
    pub(super) fn compiling(&mut self, index: usize, total: usize, path: &str) -> io::Result<()> {
        self.show("Compiling", &format!("[{index}/{total}] {path}"))
    }
    pub(super) fn running(&mut self, index: usize, total: usize, path: &str) -> io::Result<()> {
        self.show("Running", &format!("[{index}/{total}] {path}"))
    }
    fn show(&mut self, label: &str, detail: &str) -> io::Result<()> {
        if !self.enabled {
            return Ok(());
        }
        let Some(columns) = (self.width)().filter(|columns| *columns > 1) else {
            return self.clear();
        };
        let detail: String = detail
            .chars()
            .map(|ch| if ch.is_control() { ' ' } else { ch })
            .collect();
        let line = format!("{label:>12} {detail}");
        let line = fit_line(&line, usize::from(columns - 1));
        self.visible = true;
        write!(self.writer, "\r\x1b[2K{line}")?;
        self.writer.flush()
    }
    pub(super) fn clear(&mut self) -> io::Result<()> {
        if self.visible {
            self.writer.write_all(b"\r\x1b[2K")?;
            self.writer.flush()?;
            self.visible = false;
        }
        Ok(())
    }
}

fn stderr_width() -> Option<u16> {
    #[cfg(any(unix, windows))]
    {
        terminal_size::terminal_size_of(std::io::stderr())
            .map(|(terminal_size::Width(width), _)| width)
    }
    #[cfg(not(any(unix, windows)))]
    {
        None
    }
}

fn fit_line(line: &str, columns: usize) -> String {
    if line.width() <= columns {
        return line.to_owned();
    }
    let mut end = 0;
    for (offset, ch) in line.char_indices() {
        let next = offset + ch.len_utf8();
        if line[..next].width() > columns - 1 {
            break;
        }
        end = next;
    }
    format!("{}…", &line[..end])
}

impl<W: Write> Drop for TestProgress<W> {
    fn drop(&mut self) {
        // Explicit calls return output errors; Drop only cleans up during early exit.
        let _ = self.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::TestProgress;
    use std::io::{self, Write};

    #[test]
    fn width_progress_clips_by_unicode_display_width() {
        let mut output = Vec::new();
        {
            let mut progress = TestProgress::with_width_source(&mut output, true, || Some(25));
            progress
                .compiling(1, 1, "日本語の長いファイル.srt")
                .unwrap();
            progress.clear().unwrap();
        }
        assert_eq!(
            String::from_utf8(output).unwrap(),
            "\r\x1b[2K   Compiling [1/1] 日本…\r\x1b[2K"
        );
    }

    #[test]
    fn width_progress_does_not_guess_when_size_is_unavailable() {
        let mut output = Vec::new();
        {
            let mut progress = TestProgress::with_width_source(&mut output, true, || None);
            progress.preparing().unwrap();
        }
        assert!(output.is_empty());
    }

    #[test]
    fn width_progress_rechecks_size_and_clears_after_terminal_disappears() {
        let mut output = Vec::new();
        let mut widths = [Some(80), None].into_iter();
        {
            let mut progress =
                TestProgress::with_width_source(&mut output, true, move || widths.next().flatten());
            progress.preparing().unwrap();
            progress.running(1, 1, "file.srt").unwrap();
        }
        assert_eq!(
            String::from_utf8(output).unwrap(),
            "\r\x1b[2K   Preparing standard environment\r\x1b[2K"
        );
    }

    #[test]
    fn terminal_progress_replaces_one_line_and_clears_on_completion() {
        let mut output = Vec::new();
        {
            let mut progress = TestProgress::with_width_source(&mut output, true, || Some(200));
            progress.preparing().unwrap();
            progress.compiling(1, 2, "lib/tests/a.srt").unwrap();
            progress.running(1, 2, "lib/tests/a.srt").unwrap();
            progress.clear().unwrap();
            progress.clear().unwrap();
        }
        assert_eq!(
            String::from_utf8(output).unwrap(),
            concat!(
                "\r\x1b[2K   Preparing standard environment",
                "\r\x1b[2K   Compiling [1/2] lib/tests/a.srt",
                "\r\x1b[2K     Running [1/2] lib/tests/a.srt",
                "\r\x1b[2K",
            )
        );
    }

    #[test]
    fn nonterminal_progress_writes_nothing() {
        let mut output = Vec::new();
        {
            let mut progress = TestProgress::with_width_source(&mut output, false, || Some(200));
            progress.preparing().unwrap();
            progress.compiling(1, 1, "file.srt").unwrap();
            progress.running(1, 1, "file.srt").unwrap();
            progress.clear().unwrap();
        }
        assert!(output.is_empty());
    }

    #[test]
    fn unfinished_progress_clears_when_dropped() {
        let mut output = Vec::new();
        {
            let mut progress = TestProgress::with_width_source(&mut output, true, || Some(200));
            progress.running(2, 3, "file.srt").unwrap();
        }
        assert!(output.ends_with(b"\r\x1b[2K"));
    }

    #[test]
    fn file_names_cannot_add_lines_or_terminal_commands() {
        let mut output = Vec::new();
        {
            let mut progress = TestProgress::with_width_source(&mut output, true, || Some(200));
            progress.compiling(1, 1, "日本語\n\r\x1b[2J.srt").unwrap();
            progress.clear().unwrap();
        }
        assert_eq!(
            String::from_utf8(output).unwrap(),
            "\r\x1b[2K   Compiling [1/1] 日本語   [2J.srt\r\x1b[2K"
        );
    }

    struct FailedWriter;
    impl Write for FailedWriter {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed"))
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn progress_write_failure_is_returned() {
        let mut progress = TestProgress::with_width_source(FailedWriter, true, || Some(200));
        assert_eq!(
            progress.preparing().unwrap_err().kind(),
            io::ErrorKind::BrokenPipe
        );
    }
}
