use crate::{
    config::Config,
    error::CliResult,
    ui::output,
};

/// `intr config get <key>`
pub fn get(key: &str, json: bool) -> CliResult<()> {
    let config = Config::load()?;
    let value = config.get_key(key);
    if json {
        output::print_json_ok(&serde_json::json!({ "key": key, "value": value }));
    } else {
        match value {
            Some(v) => println!("{v}"),
            None    => output::print_info("(not set)"),
        }
    }
    Ok(())
}

/// `intr config set <key> <value>`
pub fn set(key: &str, value: &str, json: bool) -> CliResult<()> {
    let mut config = Config::load()?;
    config.set_key(key, value)?;
    config.save()?;
    if json {
        output::print_json_ok(&serde_json::json!({ "key": key, "value": value }));
    } else {
        output::print_success(&format!("{key} = {value}"));
    }
    Ok(())
}

/// `intr config list`
pub fn list(json: bool) -> CliResult<()> {
    let config = Config::load()?;
    let pairs = vec![
        ("auth.default_account", config.auth.default_account.clone().unwrap_or_default()),
        ("auth.api_base_url",    config.auth.api_base_url.clone().unwrap_or_else(|| "https://api.intentry.dev".into())),
        ("defaults.model",       config.defaults.model.clone().unwrap_or_default()),
    ];
    if json {
        let map: serde_json::Map<String, serde_json::Value> = pairs
            .into_iter()
            .map(|(k, v)| (k.to_string(), serde_json::Value::String(v)))
            .collect();
        output::print_json_ok(&map);
    } else {
        output::print_kv_table(
            &pairs.iter().map(|(k, v)| (*k, v.clone())).collect::<Vec<_>>(),
        );
    }
    Ok(())
}
