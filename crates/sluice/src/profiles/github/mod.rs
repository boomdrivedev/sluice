//! sluice's identity on GitHub (C.4 pushed refs: the forge's rules, which
//! only sluice's identity bypasses): a GitHub App whose key is on the
//! host. A JWT signed with the key finds the app's installation on a
//! repository and buys an installation token for it, which only sluice's
//! own git calls are given (`docs/adr/0019`).

pub mod actions;
pub mod app;
pub mod check;
pub mod fake;
pub mod forge;
pub mod kept;
pub mod keyless;
pub mod pulls;
pub mod render;
mod rest;
pub mod workflows;

pub use app::{App, Bearer, Call, Repo};
pub use forge::Forge;

use anyhow::Result;

use crate::identity::{Credentials, Identity, Withheld};

/// The username git gives GitHub with an installation token.
const TOKEN_USER: &str = "x-access-token";

impl Identity for App {
    fn name(&self) -> String {
        format!("GitHub app {}", self.id())
    }

    /// A remote on github.com, over HTTPS whatever its URL, since the
    /// app's token is no SSH key.
    fn url(&self, remote_url: &str) -> Option<String> {
        Repo::from_url(remote_url).map(|r| r.https())
    }

    fn credentials(&self, url: &str) -> Result<Credentials> {
        given(self, url, None)
    }

    /// A token in place of the refused one: one another call got since,
    /// or one bought afresh, since GitHub sometimes refuses one it has
    /// just given, and a token held since may have lost what it reached.
    fn renew(&self, url: &str, refused: &str) -> Result<Credentials> {
        given(self, url, Some(refused))
    }
}

/// Credentials for `url` with a token for its repository alone, or one in
/// place of the `refused` token. A token GitHub refused to give is
/// [`Withheld`] in its words, so a read that needed it is a read of the
/// remote refused (133; rule core.28).
fn given(app: &App, url: &str, refused: Option<&str>) -> Result<Credentials> {
    let repo =
        Repo::from_url(url).ok_or_else(|| anyhow::anyhow!("{url} is no repository on GitHub"))?;
    let token = app
        .issue(&repo, refused)
        .map_err(|e| match app::refused(&e) {
            Some(r) => Withheld {
                said: r.said.clone(),
                note: r.note.clone(),
            }
            .into(),
            None => e,
        })?;
    Ok(Credentials {
        username: TOKEN_USER.into(),
        password: token.secret,
        about: token.about,
    })
}
