use crate::{client::IntrClient, config::Config, error::CliResult, ui::output};

pub async fn run(query: &str, limit: u32, json: bool) -> CliResult<()> {
    let config = Config::load()?;
    // Search is a public endpoint - no auth required.
    let client = IntrClient::anonymous(&config);
    let resp = client.search(query, limit).await?;

    if json {
        output::print_json_ok(&resp.results);
        return Ok(());
    }

    if resp.results.is_empty() {
        output::print_info(&format!("No results for \"{}\".", query));
        return Ok(());
    }

    output::print_info(&format!(
        "Found {} result{} for \"{}\":",
        resp.total,
        if resp.total == 1 { "" } else { "s" },
        query,
    ));
    println!();
    for item in &resp.results {
        let tags = if item.tags.is_empty() {
            String::new()
        } else {
            format!("  [{}]", item.tags.join(", "))
        };
        println!("  {}/{} @ v{}{}", item.space_slug, item.slug, item.current_version, tags);
        if let Some(desc) = &item.description {
            if !desc.is_empty() {
                println!("    {}", desc);
            }
        }
    }
    Ok(())
}
