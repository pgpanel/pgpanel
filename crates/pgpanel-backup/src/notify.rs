use tracing::{info, warn};

/// Fire-and-forget webhook notification (Discord/Slack/generic).
pub async fn notify_webhook(url: &str, title: &str, body: &str) {
    if url.is_empty() {
        return;
    }
    let payload = serde_json::json!({
        "content": format!("**{title}**\n{body}"),
        "text": format!("{title}: {body}"),
        "username": "PgPanel",
    });
    match reqwest::Client::new()
        .post(url)
        .json(&payload)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => info!("backup webhook sent"),
        Ok(r) => warn!(status = %r.status(), "backup webhook non-success"),
        Err(e) => warn!(error = %e, "backup webhook failed"),
    }
}
