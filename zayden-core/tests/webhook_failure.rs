use zayden_core::{DeliveryError, WebhookFailure};

#[test]
fn webhook_failure_never_contains_the_url() {
    let source = serenity::Error::Io(std::io::Error::other(
        "https://discord.com/api/v10/webhooks/1/SECRET",
    ));

    let error = DeliveryError::Discord(WebhookFailure::from(source));

    assert!(!error.to_string().contains("/webhooks/"));
    assert!(!format!("{error:?}").contains("/webhooks/"));
}
