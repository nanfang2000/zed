//! Novel Chapters Panel
//!
//! A panel that displays the chapter hierarchy of a novel project with
//! support for volumes, chapters, drag-and-drop reordering, and version history.

use anyhow::Result;
use gpui::{
    actions, div, Action, App, AsyncWindowContext, Context, Entity, EventEmitter, Focusable, FocusHandle,
    InteractiveElement, IntoElement, ParentElement, Render, ScrollHandle, Styled, Subscription,
    Task, WeakEntity, Window, px, prelude::*,
};
use menu::Confirm;
use novel_chapter::{
    Chapter, ChapterId, ChapterStatus, NovelProject, Volume, VolumeId,
};
use std::path::PathBuf;
use std::sync::Arc;
use theme::ActiveTheme;
use ui::{
    prelude::*, ButtonStyle, Icon, IconButton, IconName, Label, ListItem, Tooltip,
};
use workspace::{Workspace, dock::{DockPosition, Panel, PanelEvent}};

actions!(
    novel_chapters_panel,
    [
        ToggleFocus,
        NewChapter,
        DeleteChapter,
        RenameChapter,
        NewVolume,
        DeleteVolume,
        RenameVolume,
        CollapseAll,
        ExpandAll,
        ToggleChapterExpanded,
        ShowVersionHistory,
        RestoreVersion,
    ]
);

pub fn init(cx: &mut App) {
    cx.observe_new(
        |workspace: &mut Workspace, _window: Option<&mut Window>, _cx: &mut Context<Workspace>| {
            workspace.register_action(|workspace, _: &ToggleFocus, window, cx| {
                workspace.toggle_panel_focus::<NovelChaptersPanel>(window, cx);
            });
        },
    )
    .detach();
}

/// Novel Chapters Panel - displays chapter tree with volumes and chapters
pub struct NovelChaptersPanel {
    focus_handle: FocusHandle,
    workspace: WeakEntity<Workspace>,
    width: Option<f32>,

    // Novel project state
    project: Option<Arc<NovelProject>>,
    expanded_volumes: Vec<VolumeId>,

    // UI state
    selected_item: Option<SelectedItem>,
    editing_item: Option<EditingItem>,

    // UI handles
    scroll_handle: ScrollHandle,
    pending_serialization: Task<Option<()>>,

    _subscriptions: Vec<Subscription>,
}

#[derive(Clone, PartialEq)]
enum SelectedItem {
    Chapter(ChapterId),
    Volume(VolumeId),
}

#[derive(Clone)]
struct EditingItem {
    item_id: ChapterId,
    original_title: String,
    is_volume: bool,
}

impl NovelChaptersPanel {
    pub fn new(workspace: &Workspace, cx: &mut Context<Self>) -> Self {
        let workspace_handle = workspace.weak_handle();
        let focus_handle = cx.focus_handle();

        Self {
            focus_handle,
            workspace: workspace_handle,
            width: None,
            project: None,
            expanded_volumes: Vec::new(),
            selected_item: None,
            editing_item: None,
            scroll_handle: ScrollHandle::default(),
            pending_serialization: Task::ready(None),
            _subscriptions: Vec::new(),
        }
    }

    pub fn load(
        workspace: WeakEntity<Workspace>,
        cx: AsyncWindowContext,
    ) -> Task<Result<Entity<Self>>> {
        cx.spawn(async move |cx| {
            let panel = workspace.update(cx, |workspace, cx| {
                cx.new(|cx| NovelChaptersPanel::new(workspace, cx))
            })?;

            // Try to detect and load novel project
            let project_path = workspace.update(cx, |workspace, app_cx| {
                let project = workspace.project();
                let worktrees = project.read(app_cx).visible_worktrees(app_cx);
                if let Some(first_worktree) = worktrees.into_iter().next() {
                    Some(first_worktree.read(app_cx).abs_path().to_string_lossy().into_owned())
                } else {
                    None
                }
            }).ok().flatten();

            if let Some(path) = project_path {
                let _ = panel.update(cx, |panel, cx| {
                    panel.load_project(PathBuf::from(path), cx);
                });
            }

            Ok(panel)
        })
    }

    /// Load a novel project
    pub fn load_project(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let project_path = path.clone();
        cx.spawn(async move |this, cx| {
            let result = NovelProject::load(project_path).await;

            this.update(cx, |this, cx: &mut Context<NovelChaptersPanel>| {
                match result {
                    Ok(project) => {
                        this.project = Some(Arc::new(project));

                        // Expand all volumes by default
                        if let Some(ref proj) = this.project {
                            for volume in &proj.volumes {
                                this.expanded_volumes.push(volume.id.clone());
                            }
                        }

                        // Select first chapter
                        if let Some(ref proj) = this.project {
                            if let Some(first_chapter) = proj.chapters.values().next() {
                                this.selected_item = Some(SelectedItem::Chapter(first_chapter.id));
                            }
                        }

                        cx.notify();
                    }
                    Err(e) => {
                        log::error!("Failed to load novel project: {}", e);
                    }
                }
            })
            .ok();
        })
        .detach();
    }

    /// Get chapters for a volume in order
    fn get_chapters_for_volume(&self, volume_id: VolumeId) -> Vec<&Chapter> {
        if let Some(ref project) = self.project {
            if let Some(volume) = project.volumes.iter().find(|v| v.id == volume_id) {
                let mut chapters: Vec<_> = volume.chapter_ids
                    .iter()
                    .filter_map(|id| project.chapters.get(id))
                    .collect();
                return chapters;
            }
        }
        Vec::new()
    }

    /// Check if a volume is expanded
    fn is_volume_expanded(&self, volume_id: VolumeId) -> bool {
        self.expanded_volumes.contains(&volume_id)
    }

    /// Toggle volume expansion
    fn toggle_volume_expanded(&mut self, volume_id: VolumeId) {
        if self.is_volume_expanded(volume_id.clone()) {
            self.expanded_volumes.retain(|id| *id != volume_id);
        } else {
            self.expanded_volumes.push(volume_id);
        }
    }

    /// Create a new chapter
    fn create_chapter(&mut self, _: &NewChapter, _window: &mut Window, cx: &mut Context<Self>) {
        let default_volume_id = match &self.project {
            Some(p) => p.volumes.first().map(|v| v.id.clone()).unwrap_or_else(|| {
                // Create default volume if none exists
                let new_volume_id = VolumeId(uuid::Uuid::new_v4());
                new_volume_id
            }),
            None => VolumeId(uuid::Uuid::new_v4()),
        };

        if let Some(ref mut project) = self.project {
            let proj = Arc::make_mut(project);
            if let Ok(chapter_id) = futures::executor::block_on(proj.create_chapter("新章节".to_string(), Some(default_volume_id.clone()))) {
                self.selected_item = Some(SelectedItem::Chapter(chapter_id));
                if !self.expanded_volumes.contains(&default_volume_id) {
                    self.expanded_volumes.push(default_volume_id);
                }
                cx.notify();
            }
        }
    }

    /// Create a new volume
    fn create_volume(&mut self, _: &NewVolume, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ref mut project) = self.project {
            let proj = Arc::make_mut(project);
            if let Ok(volume_id) = futures::executor::block_on(proj.create_volume("新卷".to_string())) {
                self.selected_item = Some(SelectedItem::Volume(volume_id.clone()));
                self.expanded_volumes.push(volume_id);
                cx.notify();
            }
        }
    }

    /// Delete selected item
    fn delete_selected(&mut self, _: &DeleteChapter, _window: &mut Window, cx: &mut Context<Self>) {
        let item_to_delete = match &self.selected_item {
            Some(item) => item.clone(),
            None => return,
        };

        if let Some(ref mut project) = self.project {
            let proj = Arc::make_mut(project);
            let result = match item_to_delete {
                SelectedItem::Chapter(id) => futures::executor::block_on(proj.delete_chapter(id)),
                SelectedItem::Volume(id) => futures::executor::block_on(proj.delete_volume(id)),
            };
            if result.is_ok() {
                self.selected_item = None;
                cx.notify();
            }
        }
    }

    /// Rename selected item
    fn start_rename(&mut self, _: &RenameChapter, _window: &mut Window, cx: &mut Context<Self>) {
        let (item_id, original_title, is_volume) = match &self.selected_item {
            Some(SelectedItem::Chapter(id)) => {
                let project = match &self.project {
                    Some(p) => p,
                    None => return,
                };
                let chapter = match project.chapters.get(id) {
                    Some(c) => c,
                    None => return,
                };
                (ChapterId(id.0), chapter.title.clone(), false)
            }
            Some(SelectedItem::Volume(id)) => {
                let project = match &self.project {
                    Some(p) => p,
                    None => return,
                };
                let volume = match project.volumes.iter().find(|v| v.id == *id) {
                    Some(v) => v,
                    None => return,
                };
                (ChapterId(volume.chapter_ids.first().map(|cid| cid.0).unwrap_or(0)), volume.title.clone(), true)
            }
            None => return,
        };

        self.editing_item = Some(EditingItem {
            item_id,
            original_title,
            is_volume,
        });
    }

    /// Complete rename
    fn complete_rename(&mut self, new_title: String, cx: &mut Context<Self>) {
        let editing = match &self.editing_item {
            Some(e) => e.clone(),
            None => return,
        };

        self.editing_item = None;

        if new_title.trim().is_empty() || new_title == editing.original_title {
            return;
        }

        if let Some(ref mut project) = self.project {
            let proj = Arc::make_mut(project);
            let _: Result<(), anyhow::Error> = if editing.is_volume {
                let volume_id = proj.volumes.iter().find(|v| {
                    v.chapter_ids.first().map(|cid| *cid == editing.item_id).unwrap_or(false)
                }).map(|v| v.id.clone()).unwrap_or_default();
                futures::executor::block_on(proj.rename_volume(volume_id, new_title.clone()))
            } else {
                futures::executor::block_on(proj.rename_chapter(editing.item_id, new_title.clone()))
            };
            cx.notify();
        }
    }

    /// Open selected chapter
    fn open_selected_chapter(&mut self, _: &Confirm, window: &mut Window, cx: &mut Context<Self>) {
        let chapter_id = match &self.selected_item {
            Some(SelectedItem::Chapter(id)) => *id,
            _ => return,
        };

        let project = match &self.project {
            Some(p) => p,
            None => return,
        };

        let chapter = match project.chapters.get(&chapter_id) {
            Some(c) => c,
            None => return,
        };

        let content_path = chapter.dir_path.join("content.md");
        self.workspace
            .update(cx, |workspace, cx| {
                workspace
                    .open_abs_path(content_path, workspace::OpenOptions::default(), window, cx)
                    .detach();
            })
            .ok();
    }

    /// Collapse all volumes
    fn collapse_all(&mut self, _: &CollapseAll, _window: &mut Window, cx: &mut Context<Self>) {
        self.expanded_volumes.clear();
        cx.notify();
    }

    /// Expand all volumes
    fn expand_all(&mut self, _: &ExpandAll, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ref project) = self.project {
            for volume in &project.volumes {
                let volume_id = volume.id.clone();
                if !self.expanded_volumes.contains(&volume_id) {
                    self.expanded_volumes.push(volume_id);
                }
            }
        }
        cx.notify();
    }

    /// Render the chapter tree
    fn render_tree(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let project = match &self.project {
            Some(p) => p,
            None => {
                return div()
                    .id("chapter-tree")
                    .size_full()
                    .child(Label::new("未加载项目").color(Color::Muted));
            }
        };

        let selected = self.selected_item.clone();

        v_flex()
            .id("chapter-tree")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll_handle)
            .children(
                project.volumes.iter().enumerate().map(|(volume_idx, volume)| {
                    let volume_id = volume.id.clone();
                    let is_expanded = self.is_volume_expanded(volume_id.clone());
                    let volume_id_for_selected = volume.id.clone();
                    let volume_selected = matches!(&selected, Some(SelectedItem::Volume(id)) if *id == volume_id_for_selected);

                    self.render_volume_item(volume_idx, volume, is_expanded, volume_selected, cx)
                })
            )
    }

    fn render_volume_item(
        &self,
        volume_idx: usize,
        volume: &Volume,
        is_expanded: bool,
        is_selected: bool,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let chapters = self.get_chapters_for_volume(volume.id.clone());

        let volume_id_for_click = volume.id.clone();
        let volume_id_for_toggle = volume.id.clone();
        let volume_idx_clone = volume_idx;

        v_flex()
            .id(format!("volume-{}", volume_idx))
            .child(
                h_flex()
                    .id("volume-header")
                    .px_2()
                    .py_1()
                    .gap_1()
                    .items_center()
                    .bg(if is_selected { cx.theme().colors().element_selection_background } else { gpui::transparent_black() })
                    .hover(|style| style.bg(cx.theme().colors().element_background))
                    .cursor_pointer()
                    .on_click(cx.listener({
                        let id_for_click = volume_id_for_click.clone();
                        let id_for_toggle = volume_id_for_toggle.clone();
                        move |this, _, _, cx| {
                            this.selected_item = Some(SelectedItem::Volume(id_for_click.clone()));
                            this.toggle_volume_expanded(id_for_toggle.clone());
                            cx.notify();
                        }
                    }))
                    .child(
                        IconButton::new(
                            format!("expand-{}", volume_idx_clone),
                            if is_expanded { IconName::ChevronDown } else { IconName::ChevronRight }
                        )
                        .icon_size(IconSize::Small)
                        .style(ButtonStyle::Subtle)
                        .on_click(cx.listener({
                            let id_for_toggle = volume_id_for_toggle.clone();
                            move |this, _, _, cx| {
                                this.toggle_volume_expanded(id_for_toggle.clone());
                                cx.notify();
                            }
                        }))
                    )
                    .child(
                        Icon::new(IconName::Book)
                            .size(IconSize::Small)
                            .color(Color::Accent)
                    )
                    .child(Label::new(volume.title.clone()))
                    .child(
                        Label::new(format!("({})", chapters.len()))
                            .size(LabelSize::XSmall)
                            .color(Color::Muted)
                    )
            )
            .when(is_expanded, |this| {
                let selected = self.selected_item.clone();
                this.children(
                    chapters.iter().map(|chapter| {
                        let chapter_id = chapter.id;
                        let chapter_selected = matches!(&selected, Some(SelectedItem::Chapter(id)) if *id == chapter_id);

                        self.render_chapter_item(chapter, 1, chapter_selected, cx)
                    })
                )
            })
    }

    fn render_chapter_item(
        &self,
        chapter: &Chapter,
        depth: usize,
        is_selected: bool,
        cx: &Context<Self>,
    ) -> ListItem {
        let chapter_id = chapter.id;
        let status_label = match chapter.status {
            ChapterStatus::NotStarted => "未开始",
            ChapterStatus::InProgress => "进行中",
            ChapterStatus::Draft => "草稿",
            ChapterStatus::Review => "审核",
            ChapterStatus::Complete => "完成",
        };

        ListItem::new(format!("chapter-{}", chapter_id.0))
            .indent_level(depth)
            .indent_step_size(px(16.0))
            .toggle_state(is_selected)
            .on_click(cx.listener(move |this, _, window, cx| {
                this.selected_item = Some(SelectedItem::Chapter(chapter_id));
                this.open_selected_chapter(&Confirm, window, cx);
            }))
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(Icon::new(IconName::File).color(Color::Muted).size(IconSize::Small))
                    .child(Label::new(chapter.title.clone()))
                    .child(div().flex_1())
                    .child(
                        Label::new(status_label)
                            .size(LabelSize::XSmall)
                            .color(match chapter.status {
                                ChapterStatus::Draft => Color::Warning,
                                ChapterStatus::Complete => Color::Success,
                                _ => Color::Muted,
                            })
                    )
                    .child(
                        Label::new(format!("{}字", Self::format_word_count(chapter.word_count)))
                            .color(Color::Muted)
                            .size(LabelSize::XSmall)
                    )
            )
    }

    fn format_word_count(count: usize) -> String {
        if count >= 10000 {
            format!("{:.1}万", count as f64 / 10000.0)
        } else if count >= 1000 {
            format!("{:.1}k", count as f64 / 1000.0)
        } else {
            count.to_string()
        }
    }

    fn render_toolbar(&self, cx: &Context<Self>) -> impl IntoElement {
        let chapter_count = self.project.as_ref()
            .map(|p| p.chapters.len())
            .unwrap_or(0);

        h_flex()
            .id("chapters-toolbar")
            .justify_between()
            .p_2()
            .border_b_1()
            .border_color(cx.theme().colors().border)
            .child(
                Label::new(format!("{} 章节", chapter_count))
                    .color(Color::Muted)
                    .size(LabelSize::Small)
            )
            .child(
                h_flex().gap_1()
                    .child(
                        IconButton::new("new-chapter", IconName::Plus)
                            .icon_size(IconSize::Small)
                            .style(ButtonStyle::Subtle)
                            .tooltip(|window, cx| Tooltip::text("新建章节")(window, cx))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.create_chapter(&NewChapter, window, cx);
                            }))
                    )
                    .child(
                        IconButton::new("new-volume", IconName::Book)
                            .icon_size(IconSize::Small)
                            .style(ButtonStyle::Subtle)
                            .tooltip(|window, cx| Tooltip::text("新建卷")(window, cx))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.create_volume(&NewVolume, window, cx);
                            }))
                    )
                    .child(div().w_px().h_4().bg(cx.theme().colors().border))
                    .child(
                        IconButton::new("collapse-all", IconName::ChevronRight)
                            .icon_size(IconSize::Small)
                            .style(ButtonStyle::Subtle)
                            .tooltip(|window, cx| Tooltip::text("折叠全部")(window, cx))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.collapse_all(&CollapseAll, window, cx);
                            }))
                    )
                    .child(
                        IconButton::new("expand-all", IconName::ChevronDown)
                            .icon_size(IconSize::Small)
                            .style(ButtonStyle::Subtle)
                            .tooltip(|window, cx| Tooltip::text("展开全部")(window, cx))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.expand_all(&ExpandAll, window, cx);
                            }))
                    )
            )
    }

    fn render_empty_state(&self, cx: &Context<Self>) -> impl IntoElement {
        v_flex()
            .justify_center()
            .items_center()
            .size_full()
            .child(
                v_flex()
                    .gap_2()
                    .child(Icon::new(IconName::Book).color(Color::Muted).size(IconSize::XLarge))
                    .child(Label::new("暂无章节").color(Color::Muted))
                    .child(
                        Button::new("create-first", "创建章节")
                            .style(ButtonStyle::Filled)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.create_chapter(&NewChapter, window, cx);
                            }))
                    )
            )
    }
}

impl Render for NovelChaptersPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let has_content = self.project.as_ref()
            .map(|p| p.volumes.iter().any(|v| !v.chapter_ids.is_empty()))
            .unwrap_or(false);

        v_flex()
            .id("novel-chapters-panel")
            .size_full()
            .bg(cx.theme().colors().panel_background)
            .child(self.render_toolbar(cx))
            .child(if has_content {
                self.render_tree(cx).into_any_element()
            } else {
                self.render_empty_state(cx).into_any_element()
            })
    }
}

impl Panel for NovelChaptersPanel {
    fn persistent_name() -> &'static str {
        "NovelChaptersPanel"
    }

    fn panel_key() -> &'static str {
        "NovelChaptersPanel"
    }

    fn position(&self, _window: &Window, _cx: &App) -> DockPosition {
        DockPosition::Left
    }

    fn position_is_valid(&self, position: DockPosition) -> bool {
        matches!(position, DockPosition::Left | DockPosition::Right)
    }

    fn set_position(&mut self, _position: DockPosition, _window: &mut Window, cx: &mut Context<Self>) {
        cx.notify();
    }

    fn size(&self, _window: &Window, _cx: &App) -> gpui::Pixels {
        self.width.map(px).unwrap_or(px(300.0))
    }

    fn set_size(&mut self, size: Option<gpui::Pixels>, _window: &mut Window, cx: &mut Context<Self>) {
        self.width = size.map(|s| f32::from(s));
        self.pending_serialization = cx.background_executor().spawn(async { None });
        cx.notify();
    }

    fn icon(&self, _window: &Window, _cx: &App) -> Option<IconName> {
        Some(IconName::Book)
    }

    fn icon_tooltip(&self, _window: &Window, _cx: &App) -> Option<&'static str> {
        Some("Novel Chapters")
    }

    fn toggle_action(&self) -> Box<dyn Action> {
        Box::new(ToggleFocus)
    }

    fn activation_priority(&self) -> u32 {
        7
    }

    fn starts_open(&self, _window: &Window, _cx: &App) -> bool {
        true
    }
}

impl EventEmitter<PanelEvent> for NovelChaptersPanel {}

impl Focusable for NovelChaptersPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}
