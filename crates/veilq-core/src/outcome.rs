/// How a run ends, as the process exit code scripts and CI gates read
/// (ADR-0003). Argument errors from the command line parser exit with 2 too.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Nothing above the threshold.
    Clean = 0,
    /// At least one finding above the threshold.
    Findings = 1,
    /// The run could not be completed: an unsupported format, an unreadable
    /// part, an invalid policy, a required detector without its model.
    /// Never a pass-through.
    Error = 2,
}

impl Outcome {
    pub fn code(self) -> u8 {
        self as u8
    }
}
