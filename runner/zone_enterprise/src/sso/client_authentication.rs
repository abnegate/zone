#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClientAuthentication {
    Basic,
    Post,
}

impl ClientAuthentication {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Basic => "client_secret_basic",
            Self::Post => "client_secret_post",
        }
    }

    pub fn choose(supported: Option<&[String]>) -> Self {
        let only_post = supported.is_some_and(|methods| {
            methods.iter().any(|method| method == Self::Post.as_str())
                && !methods.iter().any(|method| method == Self::Basic.as_str())
        });
        if only_post { Self::Post } else { Self::Basic }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn methods(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn basic_is_the_default() {
        assert_eq!(
            ClientAuthentication::choose(None),
            ClientAuthentication::Basic
        );
        assert_eq!(
            ClientAuthentication::choose(Some(&[])),
            ClientAuthentication::Basic
        );
    }

    #[test]
    fn basic_wins_when_both_are_listed() {
        let supported = methods(&["client_secret_post", "client_secret_basic"]);
        assert_eq!(
            ClientAuthentication::choose(Some(&supported)),
            ClientAuthentication::Basic
        );
    }

    #[test]
    fn post_only_when_it_is_the_only_secret_method() {
        let supported = methods(&["client_secret_post", "private_key_jwt"]);
        assert_eq!(
            ClientAuthentication::choose(Some(&supported)),
            ClientAuthentication::Post
        );
    }

    #[test]
    fn names_are_the_registered_methods() {
        assert_eq!(ClientAuthentication::Basic.as_str(), "client_secret_basic");
        assert_eq!(ClientAuthentication::Post.as_str(), "client_secret_post");
    }
}
