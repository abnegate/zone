use std::fmt;

use crate::problem::Problem;

#[derive(Debug)]
pub struct ConfigError {
    problems: Vec<Problem>,
}

impl ConfigError {
    pub(crate) fn new(problems: Vec<Problem>) -> Self {
        Self { problems }
    }

    pub fn problems(&self) -> &[Problem] {
        &self.problems
    }

    pub fn into_problems(self) -> Vec<Problem> {
        self.problems
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "the signer is not configured:")?;
        for problem in &self.problems {
            write!(formatter, "\n  - {problem}")?;
        }
        Ok(())
    }
}

impl std::error::Error for ConfigError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::variable::Variable;

    #[test]
    fn lists_every_problem_on_its_own_line() {
        let error = ConfigError::new(vec![
            Problem::Missing(Variable::WebhookSecret),
            Problem::Missing(Variable::SmtpHost),
        ]);
        assert_eq!(
            error.to_string(),
            "the signer is not configured:\n  - POLAR_WEBHOOK_SECRET is not set.\n  - SMTP_HOST is not set."
        );
    }
}
