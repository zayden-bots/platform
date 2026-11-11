use serenity::all::{
    CreateAllowedMentions,
    CreateComponent,
    CreateEmbed,
    CreateMessage,
    EditMessage,
    EditWebhookMessage,
    ExecuteWebhook,
    MessageFlags,
};

#[derive(Debug, Clone, Default)]
pub struct Post<'a> {
    pub content: Option<String>,
    pub embeds: Vec<CreateEmbed<'a>>,
    pub components: Vec<CreateComponent<'a>>,
    pub flags: Option<MessageFlags>,
    pub allowed_mentions: Option<CreateAllowedMentions<'a>>,
}

impl<'a> Post<'a> {
    #[must_use]
    pub fn content(content: impl Into<String>) -> Self {
        Self { content: Some(content.into()), ..Self::default() }
    }

    #[must_use]
    pub fn components_v2(components: Vec<CreateComponent<'a>>) -> Self {
        Self {
            components,
            flags: Some(MessageFlags::IS_COMPONENTS_V2),
            ..Self::default()
        }
    }

    #[must_use]
    pub fn embed(mut self, embed: CreateEmbed<'a>) -> Self {
        self.embeds.push(embed);
        self
    }

    #[must_use]
    pub fn allowed_mentions(mut self, mentions: CreateAllowedMentions<'a>) -> Self {
        self.allowed_mentions = Some(mentions);
        self
    }

    pub fn to_message(&self) -> CreateMessage<'a> {
        let mut message = CreateMessage::new();
        if let Some(content) = &self.content {
            message = message.content(content.clone());
        }
        if !self.embeds.is_empty() {
            message = message.embeds(self.embeds.clone());
        }
        if !self.components.is_empty() {
            message = message.components(self.components.clone());
        }
        if let Some(flags) = self.flags {
            message = message.flags(flags);
        }
        if let Some(mentions) = &self.allowed_mentions {
            message = message.allowed_mentions(mentions.clone());
        }
        message
    }

    pub fn to_edit_message(&self) -> EditMessage<'a> {
        let mut message = EditMessage::new();
        if let Some(content) = &self.content {
            message = message.content(content.clone());
        }
        if !self.embeds.is_empty() {
            message = message.embeds(self.embeds.clone());
        }
        if !self.components.is_empty() {
            message = message.components(self.components.clone());
        }
        if let Some(flags) = self.flags {
            message = message.flags(flags);
        }
        if let Some(mentions) = &self.allowed_mentions {
            message = message.allowed_mentions(mentions.clone());
        }
        message
    }

    pub fn to_webhook(&self) -> ExecuteWebhook<'a> {
        let mut message = ExecuteWebhook::new();
        if let Some(content) = &self.content {
            message = message.content(content.clone());
        }
        if !self.embeds.is_empty() {
            message = message.embeds(self.embeds.clone());
        }
        if !self.components.is_empty() {
            message =
                message.components(self.components.clone()).with_components(true);
        }
        if let Some(flags) = self.flags {
            message = message.flags(flags);
        }
        if let Some(mentions) = &self.allowed_mentions {
            message = message.allowed_mentions(mentions.clone());
        }
        message
    }

    pub fn to_webhook_edit(&self) -> EditWebhookMessage<'a> {
        let mut message = EditWebhookMessage::new();
        if let Some(content) = &self.content {
            message = message.content(content.clone());
        }
        if !self.embeds.is_empty() {
            message = message.embeds(self.embeds.clone());
        }
        if !self.components.is_empty() {
            message = message.components(self.components.clone());
        }
        if let Some(flags) = self.flags {
            message = message.flags(flags);
        }
        if let Some(mentions) = &self.allowed_mentions {
            message = message.allowed_mentions(mentions.clone());
        }
        message
    }
}
