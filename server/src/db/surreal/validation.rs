use crate::{HarpeError, Result};

pub(super) fn validate_present(label: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(HarpeError::Validation(format!("{label} is required")));
    }

    Ok(())
}
