use crate::{auth, error::CliResult, ui::output};

pub async fn run(json: bool) -> CliResult<()> {
    auth::delete_token()?;
    if json {
        output::print_json_ok(&serde_json::json!({ "logged_out": true }));
    } else {
        output::print_success("Logged out.");
    }
    Ok(())
}
