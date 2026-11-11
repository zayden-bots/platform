use serenity::all::{CreateComponent, CreateTextDisplay};
use zayden_core::Post;

#[test]
fn post_to_message_keeps_components_v2() {
    let post = Post::components_v2(vec![CreateComponent::TextDisplay(
        CreateTextDisplay::new("hello"),
    )]);

    let value = serde_json::to_value(post.to_message()).unwrap();

    assert_eq!(value["flags"], 32768);
    assert_eq!(value["components"].as_array().map(Vec::len), Some(1));
}
