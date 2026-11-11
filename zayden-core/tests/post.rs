use serenity::all::{ChannelId, CreateComponent, CreateTextDisplay, MessageId};
use zayden_core::{Post, PostPayload};

#[test]
fn webhook_post_blocks_everyone_by_default() {
    let value = serde_json::to_value(Post::content("x").to_webhook()).unwrap();

    assert_eq!(
        value["allowed_mentions"]["parse"],
        serde_json::json!(["users", "roles"])
    );
}

#[test]
fn post_payload_new_matches_queue_post_json() {
    let post = Post::content("x");
    let channel = ChannelId::new(5);

    let create = PostPayload::new(channel, None, &post).unwrap();
    assert_eq!(create.channel_id, 5);
    assert_eq!(create.edit_message_id, None);
    assert_eq!(create.message, serde_json::to_value(post.to_message()).unwrap());

    let edit = PostPayload::new(channel, Some(MessageId::new(9)), &post).unwrap();
    assert_eq!(edit.edit_message_id, Some(9));
    assert_eq!(edit.message, serde_json::to_value(post.to_edit_message()).unwrap());
}

#[test]
fn post_to_message_keeps_components_v2() {
    let post = Post::components_v2(vec![CreateComponent::TextDisplay(
        CreateTextDisplay::new("hello"),
    )]);

    let value = serde_json::to_value(post.to_message()).unwrap();

    assert_eq!(value["flags"], 32768);
    assert_eq!(value["components"].as_array().map(Vec::len), Some(1));
}
