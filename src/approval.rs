use std::future::Future;
use std::io::{BufRead, IsTerminal, Write};

use serde_json::Value;

use crate::policy::Reason;

/// What the user is asked to approve.
#[derive(Debug, Clone)]
pub struct Request<'a> {
    pub id: &'a str,
    pub site: &'a str,
    pub description: &'a str,
    pub args: &'a Value,
    pub reason: Reason,
}

impl Request<'_> {
    /// The question put to the user, in plain words.
    pub fn message(&self) -> String {
        let why = match self.reason {
            Reason::CatalogWrite => "changes things on the site",
            Reason::PageConsequential => "is marked consequential by the page",
        };
        let args = serde_json::to_string_pretty(self.args).unwrap_or_default();
        format!(
            "Allow {} on {}?\n{}\nThis tool {why}.\nArguments:\n{args}",
            self.id, self.site, self.description
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Granted,
    /// The user was asked and said no.
    Declined,
    /// There is nobody to ask: no terminal, and the client cannot prompt.
    Unavailable,
    /// The user was asked, but there was no usable answer: dismissed, timed
    /// out, or the reply could not be understood.
    Unanswered,
}

/// Asks the user for permission. Implemented per front end: the terminal, an MCP client.
pub trait Approver: Sync {
    fn ask(&self, request: &Request<'_>) -> impl Future<Output = Decision> + Send;
}

/// Approves everything. For `--yes` and tests.
pub struct AllowAll;

impl Approver for AllowAll {
    async fn ask(&self, _: &Request<'_>) -> Decision {
        Decision::Granted
    }
}

/// Approves nothing, because nobody can be asked.
pub struct NoApprover;

impl Approver for NoApprover {
    async fn ask(&self, _: &Request<'_>) -> Decision {
        Decision::Unavailable
    }
}

/// Asks on the terminal. Without a terminal there is nobody to ask.
pub struct Terminal;

impl Approver for Terminal {
    async fn ask(&self, request: &Request<'_>) -> Decision {
        if !std::io::stdin().is_terminal() || !std::io::stderr().is_terminal() {
            return Decision::Unavailable;
        }
        let message = request.message();
        tokio::task::spawn_blocking(move || {
            let mut err = std::io::stderr();
            let _ = write!(err, "{message}\n[y/N] ");
            let _ = err.flush();
            let mut line = String::new();
            match std::io::stdin().lock().read_line(&mut line) {
                Ok(_) if matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes") => Decision::Granted,
                Ok(_) => Decision::Declined,
                Err(_) => Decision::Unavailable,
            }
        })
        .await
        .unwrap_or(Decision::Unavailable)
    }
}
