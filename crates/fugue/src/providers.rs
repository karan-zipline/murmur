use std::env;

use fugue_core::config::ConfigFile;

const DEFAULT_GITHUB_GRAPHQL_URL: &str = "https://api.github.com/graphql";
const DEFAULT_LINEAR_GRAPHQL_URL: &str = "https://api.linear.app/graphql";
const DEFAULT_ANTHROPIC_API_URL: &str = "https://api.anthropic.com";
const DEFAULT_OPENAI_API_URL: &str = "https://api.openai.com";

pub fn github_token(config: &ConfigFile) -> Option<String> {
    provider_string(config, "github", &["token", "api-key", "api_key"])
        .or_else(|| env::var("GITHUB_TOKEN").ok())
        .or_else(|| env::var("GH_TOKEN").ok())
}

pub fn github_graphql_url(config: &ConfigFile) -> String {
    env::var("GITHUB_GRAPHQL_URL")
        .ok()
        .or_else(|| provider_string(config, "github", &["graphql-url", "graphql_url", "url"]))
        .unwrap_or_else(|| DEFAULT_GITHUB_GRAPHQL_URL.to_owned())
}

pub fn linear_api_key(config: &ConfigFile) -> Option<String> {
    provider_string(config, "linear", &["api-key", "api_key", "token"])
        .or_else(|| env::var("LINEAR_API_KEY").ok())
}

pub fn linear_graphql_url(config: &ConfigFile) -> String {
    env::var("LINEAR_GRAPHQL_URL")
        .ok()
        .or_else(|| provider_string(config, "linear", &["graphql-url", "graphql_url", "url"]))
        .unwrap_or_else(|| DEFAULT_LINEAR_GRAPHQL_URL.to_owned())
}

pub fn anthropic_api_key(config: &ConfigFile) -> Option<String> {
    provider_string(config, "anthropic", &["api-key", "api_key", "token"])
        .or_else(|| env::var("ANTHROPIC_API_KEY").ok())
}

pub fn anthropic_api_url(config: &ConfigFile) -> String {
    env::var("ANTHROPIC_API_URL")
        .ok()
        .or_else(|| provider_string(config, "anthropic", &["api-url", "api_url", "url"]))
        .unwrap_or_else(|| DEFAULT_ANTHROPIC_API_URL.to_owned())
}

pub fn openai_api_key(config: &ConfigFile) -> Option<String> {
    provider_string(config, "openai", &["api-key", "api_key", "token"])
        .or_else(|| env::var("OPENAI_API_KEY").ok())
}

pub fn openai_api_url(config: &ConfigFile) -> String {
    env::var("OPENAI_API_URL")
        .ok()
        .or_else(|| provider_string(config, "openai", &["api-url", "api_url", "url"]))
        .unwrap_or_else(|| DEFAULT_OPENAI_API_URL.to_owned())
}

pub fn llm_auth_provider(config: &ConfigFile) -> Option<String> {
    llm_auth_string(config, &["provider"])
}

pub fn llm_auth_model(config: &ConfigFile) -> Option<String> {
    llm_auth_string(config, &["model"])
}

fn provider_string(config: &ConfigFile, provider: &str, keys: &[&str]) -> Option<String> {
    let table = config.providers.get(provider)?.as_table()?;
    keys.iter()
        .find_map(|key| table.get(*key).and_then(|v| v.as_str()).map(str::to_owned))
        .and_then(|s| (!s.trim().is_empty()).then_some(s))
}

fn llm_auth_string(config: &ConfigFile, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| {
            config
                .llm_auth
                .get(*key)
                .and_then(|v| v.as_str())
                .map(str::to_owned)
        })
        .and_then(|s| (!s.trim().is_empty()).then_some(s))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::{Mutex, OnceLock};

    use super::*;

    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    fn with_env_lock<F: FnOnce()>(f: F) {
        let lock = ENV_LOCK.get_or_init(|| Mutex::new(()));
        let _guard = lock.lock().unwrap();
        f();
    }

    fn set_env(key: &str, value: &str) -> Option<String> {
        let prev = env::var(key).ok();
        env::set_var(key, value);
        prev
    }

    fn restore_env(key: &str, prev: Option<String>) {
        match prev {
            Some(v) => env::set_var(key, v),
            None => env::remove_var(key),
        }
    }

    #[test]
    fn github_token_prefers_config_over_env() {
        with_env_lock(|| {
            let prev = set_env("GITHUB_TOKEN", "env-token");

            let mut github = toml::value::Table::new();
            github.insert(
                "token".to_owned(),
                toml::Value::String("cfg-token".to_owned()),
            );

            let cfg = ConfigFile {
                providers: BTreeMap::from([("github".to_owned(), toml::Value::Table(github))]),
                ..Default::default()
            };

            assert_eq!(github_token(&cfg).as_deref(), Some("cfg-token"));

            restore_env("GITHUB_TOKEN", prev);
        });
    }

    #[test]
    fn github_token_falls_back_to_env() {
        with_env_lock(|| {
            let prev = set_env("GITHUB_TOKEN", "env-token");

            let cfg = ConfigFile::default();
            assert_eq!(github_token(&cfg).as_deref(), Some("env-token"));

            restore_env("GITHUB_TOKEN", prev);
        });
    }

    #[test]
    fn linear_api_key_prefers_config_over_env() {
        with_env_lock(|| {
            let prev = set_env("LINEAR_API_KEY", "env-key");

            let mut linear = toml::value::Table::new();
            linear.insert(
                "api-key".to_owned(),
                toml::Value::String("cfg-key".to_owned()),
            );

            let cfg = ConfigFile {
                providers: BTreeMap::from([("linear".to_owned(), toml::Value::Table(linear))]),
                ..Default::default()
            };

            assert_eq!(linear_api_key(&cfg).as_deref(), Some("cfg-key"));

            restore_env("LINEAR_API_KEY", prev);
        });
    }

    #[test]
    fn openai_api_key_prefers_config_over_env() {
        with_env_lock(|| {
            let prev = set_env("OPENAI_API_KEY", "env-key");

            let mut openai = toml::value::Table::new();
            openai.insert(
                "api-key".to_owned(),
                toml::Value::String("cfg-key".to_owned()),
            );

            let cfg = ConfigFile {
                providers: BTreeMap::from([("openai".to_owned(), toml::Value::Table(openai))]),
                ..Default::default()
            };

            assert_eq!(openai_api_key(&cfg).as_deref(), Some("cfg-key"));

            restore_env("OPENAI_API_KEY", prev);
        });
    }

    #[test]
    fn llm_auth_provider_reads_table_value() {
        let cfg = ConfigFile {
            llm_auth: BTreeMap::from([(
                "provider".to_owned(),
                toml::Value::String("openai".to_owned()),
            )]),
            ..Default::default()
        };

        assert_eq!(llm_auth_provider(&cfg).as_deref(), Some("openai"));
    }
}
