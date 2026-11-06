//! The token client Claude sign-ins share, built again only when the token endpoint or the proxy
//! it goes through changes.

use std::sync::Mutex;
use std::sync::PoisonError;

use abnegate_agent_cli::login::claude::Client;

/// The token URL and proxy the shared client was built for, and the client.
type Built = (String, Option<String>, Client);

pub struct Clients {
    built: Mutex<Option<Built>>,
}

impl Clients {
    pub const fn new() -> Self {
        Self {
            built: Mutex::new(None),
        }
    }

    /// The client for `token` through `proxy`: the one already built for them, or the one `build`
    /// builds, which is kept in its place.
    pub fn get<E>(
        &self,
        token: &str,
        proxy: Option<&str>,
        build: impl FnOnce() -> Result<Client, E>,
    ) -> Result<Client, E> {
        let mut built = self.built.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((built_token, built_proxy, client)) = built.as_ref()
            && built_token == token
            && built_proxy.as_deref() == proxy
        {
            return Ok(client.clone());
        }
        let client = build()?;
        *built = Some((token.to_string(), proxy.map(str::to_string), client.clone()));
        Ok(client)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::convert::Infallible;

    use abnegate_agent_cli::login::claude::Endpoints;

    use super::*;

    const TOKEN: &str = "http://127.0.0.1:4010/v1/oauth/token";
    const MOVED: &str = "http://127.0.0.1:4011/v1/oauth/token";
    const PROXY: &str = "http://proxy.internal:3128/";
    const UNBUILT: &str = "no TLS backend could start";

    fn building(builds: &Cell<usize>) -> impl FnOnce() -> Result<Client, Infallible> + '_ {
        move || {
            builds.set(builds.get() + 1);
            Ok(Client::new(Endpoints::default()).expect("a client"))
        }
    }

    #[test]
    fn a_client_is_built_once_for_the_same_token_url_and_proxy() {
        let clients = Clients::new();
        let builds = Cell::new(0);

        for _ in 0..3 {
            let Ok(_) = clients.get(TOKEN, Some(PROXY), building(&builds));
        }

        assert_eq!(builds.get(), 1);
    }

    #[test]
    fn a_changed_token_url_or_proxy_builds_a_new_client() {
        let clients = Clients::new();
        let builds = Cell::new(0);

        for (token, proxy) in [
            (TOKEN, None),
            (TOKEN, Some(PROXY)),
            (MOVED, Some(PROXY)),
            (MOVED, None),
            (MOVED, None),
        ] {
            let Ok(_) = clients.get(token, proxy, building(&builds));
        }

        assert_eq!(builds.get(), 4);
    }

    #[test]
    fn a_client_that_fails_to_build_is_not_kept() {
        let clients = Clients::new();
        let builds = Cell::new(0);

        let failed = clients.get(TOKEN, None, || Err(UNBUILT));
        let Ok(_) = clients.get(TOKEN, None, building(&builds));

        assert_eq!(failed.err(), Some(UNBUILT));
        assert_eq!(builds.get(), 1);
    }
}
