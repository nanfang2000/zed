//! Novel AI Panel
//!
//! AI-powered writing assistant panel for novel creation.

use anyhow::Result;
use futures::StreamExt;
use gpui::{
    actions, div, Action, App, AppContext, AsyncWindowContext, Entity, EventEmitter, Focusable, FocusHandle,
    InteractiveElement, IntoElement, ParentElement, Render, ScrollHandle, Styled,
    Subscription, Task, WeakEntity, Window, px, prelude::*,
};
use language_model::{LanguageModelRegistry, LanguageModelRequest, LanguageModelRequestMessage, MessageContent, Role};
use novel_chapter::{Chapter, CharacterProfile, WorldSetting};
use theme::ActiveTheme;
use ui::{
    prelude::*, Button, ButtonStyle, Icon, IconName, Label,
};
use workspace::{Workspace, dock::{DockPosition, Panel, PanelEvent}};

actions!(
    novel_ai_panel,
    [
        ToggleFocus,
        SendMessage,
        GenerateChapter,
        ContinueWriting,
        RewriteSelection,
        CheckConsistency,
        GenerateCharacter,
        SuggestPlot,
    ]
);

pub fn init(cx: &mut App) {
    cx.observe_new(
        |workspace: &mut Workspace, _window: Option<&mut Window>, _cx: &mut Context<Workspace>| {
            workspace.register_action(|workspace, _: &ToggleFocus, window, cx| {
                workspace.toggle_panel_focus::<NovelAIPanel>(window, cx);
            });
        },
    )
    .detach();
}

/// AI Panel for novel writing assistance
pub struct NovelAIPanel {
    focus_handle: FocusHandle,
    workspace: WeakEntity<Workspace>,
    width: Option<f32>,

    // Chat state
    messages: Vec<Message>,
    input_text: String,

    // Novel context
    current_chapter: Option<Chapter>,
    novel_context: Option<NovelContext>,

    // AI state
    is_generating: bool,
    pending_request: Option<Task<Result<()>>>,

    // UI state
    scroll_handle: ScrollHandle,

    _subscriptions: Vec<Subscription>,
}

#[derive(Clone, Debug)]
pub struct Message {
    pub role: MessageRole,
    pub content: String,
    pub timestamp: std::time::SystemTime,
}

#[derive(Clone, Debug, PartialEq)]
pub enum MessageRole {
    User,
    Assistant,
    System,
}

#[derive(Clone, Debug)]
pub struct NovelContext {
    pub characters: Vec<CharacterProfile>,
    pub world_settings: Vec<WorldSetting>,
    pub recent_chapters: Vec<String>,
}

/// Quick action commands for novel writing
#[derive(Clone, Debug)]
pub enum QuickAction {
    GenerateChapter,
    ContinueWriting,
    RewriteSelection,
    CheckConsistency,
    GenerateCharacter,
    SuggestPlot,
}

impl QuickAction {
    fn label(&self) -> &'static str {
        match self {
            Self::GenerateChapter => "生成章节",
            Self::ContinueWriting => "续写",
            Self::RewriteSelection => "重写",
            Self::CheckConsistency => "逻辑自查",
            Self::GenerateCharacter => "生成人物",
            Self::SuggestPlot => "剧情建议",
        }
    }

    fn icon(&self) -> IconName {
        match self {
            Self::GenerateChapter => IconName::File,
            Self::ContinueWriting => IconName::ArrowRight,
            Self::RewriteSelection => IconName::RotateCw,
            Self::CheckConsistency => IconName::Check,
            Self::GenerateCharacter => IconName::Plus,
            Self::SuggestPlot => IconName::Sparkle,
        }
    }

    fn description(&self) -> &'static str {
        match self {
            Self::GenerateChapter => "根据设定和剧情节点生成完整章节",
            Self::ContinueWriting => "基于当前内容继续创作",
            Self::RewriteSelection => "重写选中的段落",
            Self::CheckConsistency => "检查人设、剧情和世界观一致性",
            Self::GenerateCharacter => "生成角色背景和性格设定",
            Self::SuggestPlot => "提供剧情走向建议",
        }
    }
}

impl NovelAIPanel {
    pub fn new(workspace: &Workspace, cx: &mut Context<Self>) -> Self {
        let workspace_handle = workspace.weak_handle();
        let focus_handle = cx.focus_handle();

        Self {
            focus_handle,
            workspace: workspace_handle,
            width: None,
            messages: Vec::new(),
            input_text: String::new(),
            current_chapter: None,
            novel_context: None,
            is_generating: false,
            pending_request: None,
            scroll_handle: ScrollHandle::default(),
            _subscriptions: Vec::new(),
        }
    }

    pub fn load(
        workspace: WeakEntity<Workspace>,
        cx: AsyncWindowContext,
    ) -> Task<Result<Entity<Self>>> {
        cx.spawn(async move |cx| {
            workspace.update(cx, |workspace, cx| {
                cx.new(|cx| NovelAIPanel::new(workspace, cx))
            })
        })
    }

    /// Set the current chapter context
    pub fn set_chapter_context(&mut self, chapter: Chapter, cx: &mut Context<Self>) {
        self.current_chapter = Some(chapter);
        cx.notify();
    }

    /// Set novel context (characters, world, etc.)
    pub fn set_novel_context(&mut self, context: NovelContext, cx: &mut Context<Self>) {
        self.novel_context = Some(context);
        cx.notify();
    }

    /// Send a message to AI
    fn send_message(&mut self, _: &SendMessage, _window: &mut Window, cx: &mut Context<Self>) {
        let text = self.input_text.trim().to_string();
        if text.is_empty() || self.is_generating {
            return;
        }

        // Add user message
        self.messages.push(Message {
            role: MessageRole::User,
            content: text.clone(),
            timestamp: std::time::SystemTime::now(),
        });

        self.input_text.clear();
        self.is_generating = true;

        // Generate AI response
        let request = self.generate_ai_response(text, cx);
        self.pending_request = Some(request);

        cx.notify();
    }

    /// Generate AI response using real language model
    fn generate_ai_response(&self, prompt: String, cx: &mut Context<Self>) -> Task<Result<()>> {
        let context = self.build_context_prompt();

        cx.spawn(async move |this, cx| {
            // Build full prompt with context
            let system_prompt = format!(
                "你是一位专业的小说创作助手。请根据以下上下文回答用户的问题。\n\n{}",
                context
            );

            // Get the default language model
            let model = cx.update(|cx| {
                LanguageModelRegistry::read_global(cx)
                    .default_model()
            });

            let response = if let Some(model) = model {
                // Build request with messages
                let request = LanguageModelRequest {
                    thread_id: None,
                    prompt_id: None,
                    intent: None,
                    messages: vec![
                        LanguageModelRequestMessage {
                            role: Role::System,
                            content: vec![MessageContent::Text(system_prompt)],
                            cache: false,
                            reasoning_details: None,
                        },
                        LanguageModelRequestMessage {
                            role: Role::User,
                            content: vec![MessageContent::Text(prompt.clone())],
                            cache: false,
                            reasoning_details: None,
                        },
                    ],
                    tools: vec![],
                    stop: vec![],
                    temperature: Some(0.7),
                    tool_choice: None,
                    thinking_allowed: false,
                };

                // Call the AI model with streaming
                let stream = model.model.stream_completion_text(request, cx);
                match stream.await {
                    Ok(mut messages) => {
                        let mut full_response = String::new();

                        // Collect streaming response
                        while let Some(message) = messages.stream.next().await {
                            let text: String = message?;
                            full_response.push_str(&text);

                            // Update UI with streaming text
                            this.update(cx, |this, cx: &mut Context<NovelAIPanel>| {
                                if let Some(last_msg) = this.messages.last_mut() {
                                    if last_msg.role == MessageRole::Assistant {
                                        last_msg.content = full_response.clone();
                                    }
                                } else {
                                    this.messages.push(Message {
                                        role: MessageRole::Assistant,
                                        content: full_response.clone(),
                                        timestamp: std::time::SystemTime::now(),
                                    });
                                }
                                cx.notify();
                            })?;
                        }

                        full_response
                    }
                    Err(e) => {
                        format!("AI 调用失败: {}\n\n请确保:\n1. 已配置 AI 提供商\n2. API 密钥正确\n3. 网络连接正常", e)
                    }
                }
            } else {
                "未配置 AI 模型。请先在设置中配置 AI 提供商（如 OpenAI、Anthropic 等）。".to_string()
            };

            // Ensure final message is updated
            this.update(cx, |this, cx: &mut Context<NovelAIPanel>| {
                if let Some(last_msg) = this.messages.last_mut() {
                    if last_msg.role == MessageRole::Assistant {
                        last_msg.content = response;
                    }
                } else {
                    this.messages.push(Message {
                        role: MessageRole::Assistant,
                        content: response,
                        timestamp: std::time::SystemTime::now(),
                    });
                }
                this.is_generating = false;
                cx.notify();
            }).ok();

            Ok(())
        })
    }

    /// Build context prompt from novel settings
    fn build_context_prompt(&self) -> String {
        let mut context = String::new();

        if let Some(chapter) = &self.current_chapter {
            context.push_str(&format!("当前章节: {}\n", chapter.title));
        }

        if let Some(novel_context) = &self.novel_context {
            if !novel_context.characters.is_empty() {
                context.push_str("\n人物设定:\n");
                for char in &novel_context.characters {
                    context.push_str(&format!("- {}: {}\n", char.name, char.personality));
                }
            }

            if !novel_context.world_settings.is_empty() {
                context.push_str("\n世界观设定:\n");
                for setting in &novel_context.world_settings {
                    context.push_str(&format!("- {}: {}\n", setting.name, setting.description));
                }
            }
        }

        context
    }

    /// Execute a quick action
    fn execute_quick_action(&mut self, action: QuickAction, _window: &mut Window, cx: &mut Context<Self>) {
        if self.is_generating {
            return;
        }

        let prompt = match action {
            QuickAction::GenerateChapter => {
                "请根据当前的人物设定和世界观，生成下一章节的内容。要求：\n1. 保持人物性格一致\n2. 遵循世界观设定\n3. 推进主线剧情\n4. 篇幅约3000-5000字".to_string()
            }
            QuickAction::ContinueWriting => {
                "请继续上文的内容，保持风格和节奏一致。".to_string()
            }
            QuickAction::RewriteSelection => {
                "请重写当前选中的段落，使其更加生动有趣。".to_string()
            }
            QuickAction::CheckConsistency => {
                "请检查当前章节的逻辑一致性，包括：\n1. 人物性格和行为是否一致\n2. 剧情前后是否有矛盾\n3. 世界观设定是否被违反\n4. 时间线是否合理".to_string()
            }
            QuickAction::GenerateCharacter => {
                "请生成一个新角色的详细设定，包括外貌、性格、背景故事、目标和与其他角色的关系。".to_string()
            }
            QuickAction::SuggestPlot => {
                "基于当前剧情，请提供3-5个可能的剧情走向建议，说明每个走向的优缺点。".to_string()
            }
        };

        // Add as user message and generate response
        self.messages.push(Message {
            role: MessageRole::User,
            content: format!("[快捷指令: {}]\n{}", action.label(), prompt),
            timestamp: std::time::SystemTime::now(),
        });

        self.is_generating = true;
        let request = self.generate_ai_response(prompt, cx);
        self.pending_request = Some(request);

        cx.notify();
    }

    fn render_quick_actions(&self, cx: &Context<Self>) -> impl IntoElement {
        let actions = vec![
            QuickAction::GenerateChapter,
            QuickAction::ContinueWriting,
            QuickAction::RewriteSelection,
            QuickAction::CheckConsistency,
            QuickAction::GenerateCharacter,
            QuickAction::SuggestPlot,
        ];

        v_flex()
            .gap_2()
            .p_2()
            .border_b_1()
            .border_color(cx.theme().colors().border)
            .child(
                Label::new("快捷指令")
                    .size(LabelSize::Small)
                    .color(Color::Muted)
            )
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_2()
                    .children(
                        actions.into_iter().map(|action| {
                            Button::new(format!("action-{:?}", action), action.label())
                                .style(ButtonStyle::Subtle)
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.execute_quick_action(action.clone(), window, cx);
                                }))
                        })
                    )
            )
    }

    fn render_messages(&self, cx: &Context<Self>) -> impl IntoElement {
        let messages = self.messages.clone();

        v_flex()
            .id("messages")
            .flex_1()
            .overflow_y_scroll()
            .track_scroll(&self.scroll_handle)
            .p_2()
            .gap_3()
            .when(messages.is_empty(), |this| {
                this.child(
                    v_flex()
                        .justify_center()
                        .items_center()
                        .size_full()
                        .child(Icon::new(IconName::ZedAssistant).color(Color::Muted))
                        .child(
                            Label::new("开始对话")
                                .size(LabelSize::Large)
                                .color(Color::Muted)
                        )
                        .child(
                            Label::new("使用快捷指令或输入消息")
                                .size(LabelSize::Small)
                                .color(Color::Muted)
                        )
                )
            })
            .children(
                messages.iter().map(|msg| self.render_message(msg, cx))
            )
    }

    fn render_message(&self, message: &Message, cx: &Context<Self>) -> impl IntoElement {
        let is_user = message.role == MessageRole::User;

        h_flex()
            .gap_2()
            .when(!is_user, |this| this.justify_start())
            .when(is_user, |this| this.justify_end())
            .child(
                div()
                    .max_w(px(500.0))
                    .p_3()
                    .rounded_md()
                    .when(is_user, |this| {
                        this.bg(cx.theme().colors().element_background)
                    })
                    .when(!is_user, |this| {
                        this.bg(cx.theme().colors().element_hover)
                    })
                    .child(
                        v_flex()
                            .gap_1()
                            .child(
                                h_flex()
                                    .justify_between()
                                    .child(
                                        Label::new(if is_user { "你" } else { "AI助手" })
                                            .size(LabelSize::Small)
                                            .color(if is_user { Color::Accent } else { Color::Success })
                                    )
                            )
                            .child(
                                Label::new(message.content.clone())
                                    .size(LabelSize::Default)
                            )
                    )
            )
    }

    fn render_input(&self, cx: &Context<Self>) -> impl IntoElement {
        h_flex()
            .p_2()
            .gap_2()
            .border_t_1()
            .border_color(cx.theme().colors().border)
            .child(
                div()
                    .flex_1()
                    .p_2()
                    .bg(cx.theme().colors().editor_background)
                    .rounded_md()
                    .child(Label::new("输入消息或问题...").color(Color::Muted))
            )
            .child(
                Button::new("send", "发送")
                    .style(ButtonStyle::Filled)
                    .disabled(self.is_generating)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.send_message(&SendMessage, window, cx);
                    }))
            )
    }

    fn render_status(&self, cx: &Context<Self>) -> impl IntoElement {
        h_flex()
            .p_2()
            .border_t_1()
            .border_color(cx.theme().colors().border)
            .justify_between()
            .child(
                h_flex()
                    .gap_2()
                    .when(self.current_chapter.is_some(), |this| {
                        let chapter = self.current_chapter.as_ref().unwrap();
                        this.child(
                            Label::new(format!("章节: {}", chapter.title))
                                .size(LabelSize::Small)
                                .color(Color::Muted)
                        )
                    })
            )
            .when(self.is_generating, |this| {
                this.child(
                    Label::new("AI 生成中...")
                        .size(LabelSize::Small)
                        .color(Color::Accent)
                )
            })
    }
}

impl Render for NovelAIPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .id("novel-ai-panel")
            .size_full()
            .bg(cx.theme().colors().panel_background)
            .child(self.render_quick_actions(cx))
            .child(self.render_messages(cx))
            .child(self.render_input(cx))
            .child(self.render_status(cx))
    }
}

impl Panel for NovelAIPanel {
    fn persistent_name() -> &'static str {
        "NovelAIPanel"
    }

    fn panel_key() -> &'static str {
        "NovelAIPanel"
    }

    fn position(&self, _window: &Window, _cx: &App) -> DockPosition {
        DockPosition::Right
    }

    fn position_is_valid(&self, position: DockPosition) -> bool {
        matches!(position, DockPosition::Right | DockPosition::Bottom)
    }

    fn set_position(&mut self, _position: DockPosition, _window: &mut Window, cx: &mut Context<Self>) {
        cx.notify();
    }

    fn size(&self, _window: &Window, _cx: &App) -> gpui::Pixels {
        self.width.map(px).unwrap_or(px(400.0))
    }

    fn set_size(&mut self, size: Option<gpui::Pixels>, _window: &mut Window, cx: &mut Context<Self>) {
        self.width = size.map(|s| f32::from(s));
        cx.notify();
    }

    fn icon(&self, _window: &Window, _cx: &App) -> Option<IconName> {
        Some(IconName::ZedAssistant)
    }

    fn icon_tooltip(&self, _window: &Window, _cx: &App) -> Option<&'static str> {
        Some("Novel AI Assistant")
    }

    fn toggle_action(&self) -> Box<dyn Action> {
        Box::new(ToggleFocus)
    }

    fn activation_priority(&self) -> u32 {
        10 // Higher than all other panels (project=0, terminal=1, git=2, agent=3, agents=4, outline=5, collab=6, chapters=7, notification=8, debugger=9)
    }

    fn starts_open(&self, _window: &Window, _cx: &App) -> bool {
        true
    }
}

impl EventEmitter<PanelEvent> for NovelAIPanel {}

impl Focusable for NovelAIPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}
