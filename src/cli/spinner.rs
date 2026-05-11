use std::{io::IsTerminal, time::Duration};

use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};

const SNOWFLAKE_TICKS: &[&str] = &["❄️", "❅", "❆", "✻", "✼", "✽"];

/// A small, stderr-only spinner for interactive waits.
///
/// The spinner is intentionally disabled for non-TTY use and common automation
/// environments so stdout remains machine-readable and stderr remains clean in
/// CI/log capture.
pub struct SnowflakeSpinner {
    progress: Option<ProgressBar>,
}

impl SnowflakeSpinner {
    pub fn start(message: impl Into<String>) -> Self {
        if !should_show_spinner(std::io::stderr().is_terminal(), current_env_var) {
            return Self { progress: None };
        }

        let progress = ProgressBar::new_spinner();
        progress.set_draw_target(ProgressDrawTarget::stderr());

        if let Ok(style) = ProgressStyle::with_template("{spinner} {msg}") {
            progress.set_style(style.tick_strings(SNOWFLAKE_TICKS));
        }

        progress.set_message(message.into());
        progress.enable_steady_tick(Duration::from_millis(120));

        Self {
            progress: Some(progress),
        }
    }
}

impl Drop for SnowflakeSpinner {
    fn drop(&mut self) {
        if let Some(progress) = self.progress.take() {
            progress.finish_and_clear();
        }
    }
}

fn current_env_var(name: &str) -> Result<String, std::env::VarError> {
    std::env::var(name)
}

fn should_show_spinner(
    stderr_is_tty: bool,
    env_var: impl Fn(&str) -> Result<String, std::env::VarError>,
) -> bool {
    if !stderr_is_tty {
        return false;
    }

    if env_is_set(&env_var, "SNOW_CLI_NO_SPINNER")
        || env_is_set(&env_var, "CI")
        || env_is_set(&env_var, "NO_COLOR")
    {
        return false;
    }

    !matches!(env_var("TERM").ok().as_deref(), Some("dumb"))
}

fn env_is_set(env_var: &impl Fn(&str) -> Result<String, std::env::VarError>, name: &str) -> bool {
    env_var(name).is_ok_and(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_with<'a>(
        pairs: &'a [(&'a str, &'a str)],
    ) -> impl Fn(&str) -> Result<String, std::env::VarError> + 'a {
        move |name| {
            pairs
                .iter()
                .find_map(|(key, value)| (*key == name).then(|| (*value).to_string()))
                .ok_or(std::env::VarError::NotPresent)
        }
    }

    #[test]
    fn spinner_is_hidden_when_stderr_is_not_tty() {
        assert!(!should_show_spinner(false, env_with(&[])));
    }

    #[test]
    fn spinner_is_shown_for_interactive_terminal() {
        assert!(should_show_spinner(
            true,
            env_with(&[("TERM", "xterm-256color")])
        ));
    }

    #[test]
    fn spinner_is_hidden_in_ci() {
        assert!(!should_show_spinner(true, env_with(&[("CI", "true")])));
    }

    #[test]
    fn spinner_is_hidden_when_disabled_explicitly() {
        assert!(!should_show_spinner(
            true,
            env_with(&[("SNOW_CLI_NO_SPINNER", "1")])
        ));
    }

    #[test]
    fn spinner_is_hidden_for_dumb_terminal() {
        assert!(!should_show_spinner(true, env_with(&[("TERM", "dumb")])));
    }
}
