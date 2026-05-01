//! `intr pull` - pull remote events and print a summary.
//!
//! For V1 this is a read-only preview: it fetches the remote event stream and
//! prints what is available.  Full merge / apply logic will land in V1.5.

use crate::{
    auth,
    client::IntrClient,
    config::Config,
    error::CliResult,
    ui::output,
};

pub async fn run(json: bool) -> CliResult<()> {
    let config = Config::load()?;
    let token = auth::require_token()?;
    let client = IntrClient::new(&config, token);

    output::print_info("Fetching remote events…");
    let page = client.pull_events(None).await?;

    if json {
        output::print_json_ok(&page.items);
        return Ok(());
    }

    if page.items.is_empty() {
        output::print_info("Remote is up to date.");
        return Ok(());
    }

    println!();
    for ev in &page.items {
        println!(
            "  {} {} {}",
            ev.created_at.format("%Y-%m-%d %H:%M"),
            ev.event_type,
            ev.space_id.as_deref().unwrap_or("-"),
        );
    }
    println!();
    output::print_info(&format!(
        "Fetched {} event{}{}.",
        page.items.len(),
        if page.items.len() == 1 { "" } else { "s" },
        if page.next_cursor.is_some() { " (more available)" } else { "" },
    ));

    Ok(())
}
