use crate::{auth, client::IntrClient, config::Config, error::CliResult, ui::output};

pub async fn run(json: bool) -> CliResult<()> {
    let config = Config::load()?;
    let token = auth::require_token()?;
    let client = IntrClient::new(&config, token);
    let me = client.get_me().await?;

    if json {
        output::print_json_ok(&serde_json::json!({
            "id": me.id,
            "username": me.username,
            "email": me.email,
            "display_name": me.display_name,
            "avatar_url": me.avatar_url,
        }));
    } else {
        output::print_kv_table(&[
            ("username", me.username),
            ("email", me.email),
            ("id", me.id),
        ]);
    }
    Ok(())
}
