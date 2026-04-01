#[derive(Debug, Clone)]
pub struct RuntimeError {
    pub message: String,
}

pub fn format_runtime_error(err: &RuntimeError) -> String {
    format!("RuntimeError: {}", err.message)
}

pub fn report_runtime_error(err: &RuntimeError) {
    eprintln!("{}", format_runtime_error(err));
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", format_runtime_error(self))
    }
}

impl std::error::Error for RuntimeError {}

#[cfg(test)]
mod tests {
    use super::{format_runtime_error, RuntimeError};

    #[test]
    fn format_runtime_error_uses_shared_shape() {
        let err = RuntimeError {
            message: "boom".into(),
        };
        assert_eq!(format_runtime_error(&err), "RuntimeError: boom");
    }
}
