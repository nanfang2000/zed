//! Novel Chapter Management System
//!
//! This crate provides the core data structures and file management for NovelZed's chapter system.
//!
//! Storage structure:
//! project/
//! ├── .novel/
//! │   ├── project.json          # Project metadata with volume structure
//! │   ├── settings.json         # Novel settings
//! │   ├── characters.json
//! │   ├── world.json
//! │   └── plot.json
//! ├── chapters/
//! │   └── [volume_id]/
//! │       ├── chapter-id.json   # Chapter metadata
//! │       ├── content.md        # Current content
//! │       └── history/
//! │           ├── v1.json       # Version history
//! │           ├── v2.json
//! │           └── ...
//!

use anyhow::{Context as _, Result};
use collections::HashMap;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use uuid::Uuid;

/// Unique identifier for a chapter
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChapterId(pub u64);

/// Unique identifier for a volume
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct VolumeId(pub Uuid);

impl Default for VolumeId {
    fn default() -> Self {
        VolumeId(Uuid::new_v4())
    }
}

/// A volume containing multiple chapters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Volume {
    /// Unique volume identifier
    pub id: VolumeId,
    /// Volume title
    pub title: String,
    /// Order in the novel (0-indexed)
    pub order: usize,
    /// Chapter IDs in this volume (ordered)
    pub chapter_ids: Vec<ChapterId>,
    /// Volume description
    pub description: String,
    /// Creation time
    pub created_at: SystemTime,
    /// Last modification time
    pub modified_at: SystemTime,
}

/// A novel project containing multiple volumes and settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NovelProject {
    /// Root directory of the novel project
    pub root_path: PathBuf,
    /// Project title
    pub title: String,
    /// List of volumes in order
    pub volumes: Vec<Volume>,
    /// Map of chapter IDs to chapters (flat storage for easy access)
    pub chapters: HashMap<ChapterId, Chapter>,
    /// Novel-specific settings (characters, world, plot)
    pub settings: NovelSettings,
    /// Project creation time
    pub created_at: SystemTime,
    /// Last modification time
    pub modified_at: SystemTime,
}

/// Chapter status for tracking progress
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChapterStatus {
    /// Not started
    NotStarted,
    /// Currently writing
    InProgress,
    /// First draft complete
    Draft,
    /// Under review
    Review,
    /// Finalized
    Complete,
}

/// A version snapshot of a chapter
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChapterVersion {
    /// Version number (incrementing)
    pub version: u32,
    /// Content at this version
    pub content: String,
    /// Word count at this version
    pub word_count: usize,
    /// Summary of changes
    pub summary: String,
    /// Timestamp of this version
    pub timestamp: SystemTime,
}

/// A single chapter in the novel
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chapter {
    /// Unique identifier
    pub id: ChapterId,
    /// Chapter title
    pub title: String,
    /// Order within volume (0-indexed)
    pub order: usize,
    /// Volume this chapter belongs to
    pub volume_id: VolumeId,
    /// Directory path for this chapter
    pub dir_path: PathBuf,
    /// Current content
    pub content: String,
    /// Word count (cached)
    pub word_count: usize,
    /// Chapter status
    pub status: ChapterStatus,
    /// Current version number
    pub current_version: u32,
    /// Creation time
    pub created_at: SystemTime,
    /// Last modification time
    pub modified_at: SystemTime,
}

/// Novel settings including characters, world, and plot
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NovelSettings {
    /// Character profiles
    pub characters: Vec<CharacterProfile>,
    /// World building settings
    pub world: Vec<WorldSetting>,
    /// Plot points and story structure
    pub plot_points: Vec<PlotPoint>,
}

/// Character profile
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CharacterProfile {
    /// Character name
    pub name: String,
    /// Age
    pub age: Option<u32>,
    /// Physical description
    pub appearance: String,
    /// Personality traits
    pub personality: String,
    /// Background story
    pub background: String,
    /// Character goals
    pub goals: String,
    /// Relationships with other characters
    pub relationships: HashMap<String, String>,
}

/// World setting entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldSetting {
    /// Setting name (e.g., "Magic System", "Geography")
    pub name: String,
    /// Detailed description
    pub description: String,
    /// Rules and constraints
    pub rules: Vec<String>,
}

/// Plot point for story structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlotPoint {
    /// Plot point title
    pub title: String,
    /// Detailed description
    pub description: String,
    /// Associated chapter IDs
    pub chapter_ids: Vec<ChapterId>,
    /// Order in the overall plot
    pub order: usize,
}

impl NovelProject {
    /// Create a new novel project
    pub fn new(root_path: PathBuf, title: String) -> Self {
        let now = SystemTime::now();
        let default_volume_id = VolumeId::default();

        Self {
            root_path,
            title,
            volumes: vec![Volume {
                id: default_volume_id,
                title: "第一卷".to_string(),
                order: 0,
                chapter_ids: Vec::new(),
                description: String::new(),
                created_at: now,
                modified_at: now,
            }],
            chapters: HashMap::default(),
            settings: NovelSettings::default(),
            created_at: now,
            modified_at: now,
        }
    }

    /// Initialize project directory structure
    pub async fn initialize(&self) -> Result<()> {
        let root = &self.root_path;

        // Create directories
        std::fs::create_dir_all(root.join(".novel"))?;
        std::fs::create_dir_all(root.join("chapters"))?;
        std::fs::create_dir_all(root.join("drafts"))?;

        // Save project metadata
        self.save_metadata().await?;

        Ok(())
    }

    /// Load a novel project from a directory
    pub async fn load(root_path: PathBuf) -> Result<Self> {
        let project_file = root_path.join(".novel/project.json");
        let content = std::fs::read_to_string(&project_file)
            .context("Failed to read project file")?;

        let mut project: NovelProject = serde_json::from_str(&content)
            .context("Failed to parse project file")?;

        project.root_path = root_path;

        // Load chapters from disk
        project.reload_chapters().await?;

        Ok(project)
    }

    /// Save project metadata to disk
    pub async fn save_metadata(&self) -> Result<()> {
        let project_file = self.root_path.join(".novel/project.json");
        let content = serde_json::to_string_pretty(self)
            .context("Failed to serialize project")?;

        std::fs::write(&project_file, content)
            .context("Failed to write project file")?;

        // Save settings
        self.save_settings().await?;

        Ok(())
    }

    /// Save novel settings to disk
    async fn save_settings(&self) -> Result<()> {
        let characters_file = self.root_path.join(".novel/characters.json");
        let world_file = self.root_path.join(".novel/world.json");
        let plot_file = self.root_path.join(".novel/plot.json");

        std::fs::write(
            characters_file,
            serde_json::to_string_pretty(&self.settings.characters)?,
        )?;

        std::fs::write(
            world_file,
            serde_json::to_string_pretty(&self.settings.world)?,
        )?;

        std::fs::write(
            plot_file,
            serde_json::to_string_pretty(&self.settings.plot_points)?,
        )?;

        Ok(())
    }

    /// Reload chapters from disk
    async fn reload_chapters(&mut self) -> Result<()> {
        let chapters_dir = self.root_path.join("chapters");

        if !chapters_dir.exists() {
            return Ok(());
        }

        let entries = std::fs::read_dir(&chapters_dir)
            .context("Failed to read chapters directory")?;

        for entry in entries {
            let entry = entry?;
            let path = entry.path();

            if path.is_dir() {
                if let Some(chapter) = self.load_chapter_directory(&path).await? {
                    self.chapters.insert(chapter.id, chapter);
                }
            }
        }

        Ok(())
    }

    /// Load a chapter from its directory
    async fn load_chapter_directory(&self, dir_path: &Path) -> Result<Option<Chapter>> {
        let metadata_file = dir_path.join("metadata.json");

        if !metadata_file.exists() {
            return Ok(None);
        }

        let content = std::fs::read_to_string(&metadata_file)
            .context("Failed to read chapter metadata")?;

        let mut chapter: Chapter = serde_json::from_str(&content)
            .context("Failed to parse chapter metadata")?;

        // Load current content
        let content_file = dir_path.join("content.md");
        if content_file.exists() {
            chapter.content = std::fs::read_to_string(&content_file)?;
            chapter.word_count = chapter.content.split_whitespace().count();
        }

        chapter.dir_path = dir_path.to_path_buf();
        chapter.current_version = self.get_latest_version(dir_path)?;

        Ok(Some(chapter))
    }

    /// Get the latest version number for a chapter
    fn get_latest_version(&self, dir_path: &Path) -> Result<u32> {
        let history_dir = dir_path.join("history");
        if !history_dir.exists() {
            return Ok(0);
        }

        let entries = std::fs::read_dir(&history_dir)
            .context("Failed to read history directory")?;

        let mut max_version = 0u32;
        for entry in entries {
            let entry = entry?;
            let filename = entry.file_name();
            if let Some(name) = filename.to_str() {
                if name.starts_with("v") && name.ends_with(".json") {
                    let version_str = &name[1..name.len() - 5]; // Remove "v" and ".json"
                    if let Ok(v) = version_str.parse::<u32>() {
                        max_version = max_version.max(v);
                    }
                }
            }
        }

        Ok(max_version)
    }

    /// Get all chapters in order (flattened from volumes)
    pub fn get_all_chapters_in_order(&self) -> Vec<&Chapter> {
        let mut chapters: Vec<_> = self.chapters.values().collect();
        chapters.sort_by_key(|c| {
            let volume = self.volumes.iter().find(|v| v.id == c.volume_id);
            (volume.map_or(0, |v| v.order), c.order)
        });
        chapters
    }

    /// Create a new volume
    pub async fn create_volume(&mut self, title: String) -> Result<VolumeId> {
        let id = VolumeId::default();
        let order = self.volumes.len();
        let id_clone = id.clone();

        let now = SystemTime::now();
        let volume = Volume {
            id,
            title,
            order,
            chapter_ids: Vec::new(),
            description: String::new(),
            created_at: now,
            modified_at: now,
        };

        self.volumes.push(volume);
        self.modified_at = now;

        self.save_metadata().await?;
        Ok(id_clone)
    }

    /// Delete a volume
    pub async fn delete_volume(&mut self, id: VolumeId) -> Result<()> {
        if let Some(index) = self.volumes.iter().position(|v| v.id == id) {
            let volume = &self.volumes[index];

            // Delete all chapters in this volume
            for chapter_id in &volume.chapter_ids {
                if let Some(chapter) = self.chapters.remove(chapter_id) {
                    self.delete_chapter_files(&chapter)?;
                }
            }

            // Remove volume
            self.volumes.remove(index);

            // Update order for remaining volumes
            for (new_order, volume) in self.volumes.iter_mut().enumerate() {
                volume.order = new_order;
            }

            self.modified_at = SystemTime::now();
            self.save_metadata().await?;
        }

        Ok(())
    }

    /// Rename a volume
    pub async fn rename_volume(&mut self, id: VolumeId, new_title: String) -> Result<()> {
        if let Some(volume) = self.volumes.iter_mut().find(|v| v.id == id) {
            volume.title = new_title;
            volume.modified_at = SystemTime::now();
            self.modified_at = SystemTime::now();
            self.save_metadata().await?;
        }

        Ok(())
    }

    /// Create a new chapter
    pub async fn create_chapter(&mut self, title: String, volume_id: Option<VolumeId>) -> Result<ChapterId> {
        let volume_id = volume_id.unwrap_or_else(|| self.volumes.first().map_or(VolumeId::default(), |v| v.id.clone()));

        let volume = self.volumes.iter_mut().find(|v| v.id == volume_id)
            .context("Volume not found")?;

        let order = volume.chapter_ids.len();
        let id = ChapterId(self.chapters.len() as u64);

        let chapter_dir = self.root_path.join("chapters").join(format!("chapter-{}", id.0));
        std::fs::create_dir_all(&chapter_dir)?;

        let now = SystemTime::now();
        let chapter = Chapter {
            id,
            title: title.clone(),
            order,
            volume_id,
            dir_path: chapter_dir.clone(),
            content: String::new(),
            word_count: 0,
            status: ChapterStatus::NotStarted,
            current_version: 0,
            created_at: now,
            modified_at: now,
        };

        // Save chapter metadata before insert (avoids borrow conflict)
        let chapter_dir_clone = chapter_dir.clone();
        Self::save_chapter_metadata(&chapter, chapter_dir_clone)?;

        // Save empty content
        let content_file = chapter_dir.join("content.md");
        std::fs::write(&content_file, "")?;

        // Add to storage and volume
        self.chapters.insert(id, chapter.clone());
        volume.chapter_ids.push(id);
        volume.modified_at = now;
        self.modified_at = now;

        self.save_metadata().await?;

        Ok(id)
    }

    /// Save chapter metadata
    fn save_chapter_metadata(chapter: &Chapter, dir_path: PathBuf) -> Result<()> {
        let metadata_file = dir_path.join("metadata.json");
        let content = serde_json::to_string_pretty(chapter)
            .context("Failed to serialize chapter")?;
        std::fs::write(&metadata_file, content)?;
        Ok(())
    }

    /// Delete a chapter
    pub async fn delete_chapter(&mut self, id: ChapterId) -> Result<()> {
        if let Some(chapter) = self.chapters.remove(&id) {
            // Remove from volume
            for volume in &mut self.volumes {
                if let Some(pos) = volume.chapter_ids.iter().position(|cid| *cid == id) {
                    volume.chapter_ids.remove(pos);

                    // Update order for remaining chapters in this volume
                    for (new_order, chapter_id) in volume.chapter_ids.iter_mut().enumerate() {
                        if let Some(ch) = self.chapters.get_mut(chapter_id) {
                            ch.order = new_order;
                        }
                    }
                    break;
                }
            }

            // Delete files
            self.delete_chapter_files(&chapter)?;

            self.modified_at = SystemTime::now();
            self.save_metadata().await?;
        }

        Ok(())
    }

    /// Delete chapter files from disk
    fn delete_chapter_files(&self, chapter: &Chapter) -> Result<()> {
        if chapter.dir_path.exists() {
            std::fs::remove_dir_all(&chapter.dir_path)
                .context("Failed to delete chapter directory")?;
        }
        Ok(())
    }

    /// Rename a chapter
    pub async fn rename_chapter(&mut self, id: ChapterId, new_title: String) -> Result<()> {
        if let Some(chapter) = self.chapters.get_mut(&id) {
            chapter.title = new_title.clone();
            chapter.modified_at = SystemTime::now();

            // Save metadata with path clone to avoid borrow conflict
            let dir_path = chapter.dir_path.clone();
            Self::save_chapter_metadata(chapter, dir_path)?;

            self.modified_at = SystemTime::now();
            self.save_metadata().await?;
        }

        Ok(())
    }

    /// Update chapter content and create a version snapshot
    pub async fn update_chapter_content(
        &mut self,
        id: ChapterId,
        new_content: String,
        change_summary: Option<String>,
    ) -> Result<()> {
        if let Some(chapter) = self.chapters.get_mut(&id) {
            // Save current content as a version if it has changed
            if !chapter.content.is_empty() && chapter.content != new_content {
                let chapter_clone = chapter.clone();
                Self::save_version(&chapter_clone, chapter.content.clone(), change_summary.clone(), chapter.dir_path.clone()).await?;
            }

            // Update content
            chapter.content = new_content.clone();
            chapter.word_count = new_content.split_whitespace().count();
            chapter.modified_at = SystemTime::now();
            chapter.current_version += 1;

            // Save content file
            let content_file = chapter.dir_path.join("content.md");
            std::fs::write(&content_file, &new_content)?;

            // Save metadata with path clone to avoid borrow conflict
            let dir_path = chapter.dir_path.clone();
            Self::save_chapter_metadata(chapter, dir_path)?;

            self.modified_at = SystemTime::now();
        }

        Ok(())
    }

    /// Save a version snapshot
    async fn save_version(chapter: &Chapter, content: String, summary: Option<String>, dir_path: PathBuf) -> Result<()> {
        let history_dir = dir_path.join("history");
        std::fs::create_dir_all(&history_dir)?;

        let version = ChapterVersion {
            version: chapter.current_version,
            content,
            word_count: chapter.word_count,
            summary: summary.unwrap_or_else(|| "自动保存".to_string()),
            timestamp: SystemTime::now(),
        };

        let version_file = history_dir.join(format!("v{}.json", version.version));
        let content = serde_json::to_string_pretty(&version)
            .context("Failed to serialize version")?;
        std::fs::write(&version_file, content)?;

        Ok(())
    }

    /// Get version history for a chapter
    pub async fn get_version_history(&self, id: ChapterId) -> Result<Vec<ChapterVersion>> {
        let chapter = self.chapters.get(&id)
            .context("Chapter not found")?;

        let history_dir = chapter.dir_path.join("history");
        if !history_dir.exists() {
            return Ok(Vec::new());
        }

        let mut versions: Vec<ChapterVersion> = Vec::new();

        let entries = std::fs::read_dir(&history_dir)
            .context("Failed to read history directory")?;

        for entry in entries {
            let entry = entry?;
            let path = entry.path();

            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                let content = std::fs::read_to_string(&path)?;
                let version: ChapterVersion = serde_json::from_str(&content)?;
                versions.push(version);
            }
        }

        // Sort by version number descending
        versions.sort_by_key(|v| std::cmp::Reverse(v.version));

        Ok(versions)
    }

    /// Restore a chapter to a previous version
    pub async fn restore_version(&mut self, id: ChapterId, version: u32) -> Result<()> {
        let chapter = self.chapters.get(&id)
            .context("Chapter not found")?;

        let history_dir = chapter.dir_path.join("history");
        let version_file = history_dir.join(format!("v{}.json", version));

        if !version_file.exists() {
            anyhow::bail!("Version {} not found", version);
        }

        let content = std::fs::read_to_string(&version_file)?;
        let version_data: ChapterVersion = serde_json::from_str(&content)?;

        // Update chapter with restored content
        self.update_chapter_content(
            id,
            version_data.content,
            Some(format!("恢复到版本 {}", version)),
        ).await?;

        Ok(())
    }

    /// Reorder chapters within a volume
    pub async fn reorder_chapters_in_volume(
        &mut self,
        volume_id: VolumeId,
        new_order: Vec<ChapterId>,
    ) -> Result<()> {
        if let Some(volume) = self.volumes.iter_mut().find(|v| v.id == volume_id) {
            // Validate all chapter IDs belong to this volume
            for id in &new_order {
                if let Some(chapter) = self.chapters.get(id) {
                    if chapter.volume_id != volume_id {
                        anyhow::bail!("Chapter {:?} does not belong to volume {:?}", id, volume_id);
                    }
                } else {
                    anyhow::bail!("Chapter {:?} not found", id);
                }
            }

            volume.chapter_ids = new_order;

            // Update order for all chapters
            for (new_order, chapter_id) in volume.chapter_ids.iter().enumerate() {
                if let Some(chapter) = self.chapters.get_mut(chapter_id) {
                    chapter.order = new_order;
                }
            }

            volume.modified_at = SystemTime::now();
            self.modified_at = SystemTime::now();
            self.save_metadata().await?;
        }

        Ok(())
    }

    /// Move a chapter to a different volume
    pub async fn move_chapter_to_volume(
        &mut self,
        chapter_id: ChapterId,
        target_volume_id: VolumeId,
        target_position: usize,
    ) -> Result<()> {
        let chapter = self.chapters.get(&chapter_id)
            .context("Chapter not found")?;

        let source_volume_id = chapter.volume_id.clone();

        // Remove from source volume
        for volume in &mut self.volumes {
            if volume.id == source_volume_id {
                if let Some(pos) = volume.chapter_ids.iter().position(|id| *id == chapter_id) {
                    volume.chapter_ids.remove(pos);
                    break;
                }
            }
        }

        // Add to target volume
        for volume in &mut self.volumes {
            if volume.id == target_volume_id {
                volume.chapter_ids.insert(target_position.min(volume.chapter_ids.len()), chapter_id);
                break;
            }
        }

        // Update chapter's volume_id
        if let Some(chapter) = self.chapters.get_mut(&chapter_id) {
            chapter.volume_id = target_volume_id;
        }

        self.modified_at = SystemTime::now();
        self.save_metadata().await?;

        Ok(())
    }

    /// Update chapter status
    pub async fn update_chapter_status(&mut self, id: ChapterId, status: ChapterStatus) -> Result<()> {
        if let Some(chapter) = self.chapters.get_mut(&id) {
            chapter.status = status;
            chapter.modified_at = SystemTime::now();
            let dir_path = chapter.dir_path.clone();
            Self::save_chapter_metadata(chapter, dir_path)?;
            self.modified_at = SystemTime::now();
        }

        Ok(())
    }

    /// Get chapters for a specific volume
    pub fn get_chapters_for_volume(&self, volume_id: VolumeId) -> Vec<&Chapter> {
        let mut chapters: Vec<_> = self.volumes.iter()
            .find(|v| v.id == volume_id)
            .map(|v| {
                v.chapter_ids.iter()
                    .filter_map(|id| self.chapters.get(id))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        chapters
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_create_novel_project() {
        let temp_dir = TempDir::new().unwrap();
        let root_path = temp_dir.path().to_path_buf();

        let project = NovelProject::new(root_path.clone(), "Test Novel".to_string());
        project.initialize().await.unwrap();

        assert!(root_path.join(".novel").exists());
        assert!(root_path.join("chapters").exists());
        assert!(root_path.join("drafts").exists());
        assert_eq!(project.volumes.len(), 1);
    }

    #[tokio::test]
    async fn test_create_and_load_chapter() {
        let temp_dir = TempDir::new().unwrap();
        let root_path = temp_dir.path().to_path_buf();

        let mut project = NovelProject::new(root_path.clone(), "Test Novel".to_string());
        project.initialize().await.unwrap();

        let chapter_id = project.create_chapter("First Chapter".to_string(), None).await.unwrap();

        assert_eq!(project.chapters.len(), 1);
        let chapter = project.chapters.get(&chapter_id).unwrap();
        assert_eq!(chapter.id, chapter_id);
        assert_eq!(chapter.title, "First Chapter");
    }

    #[tokio::test]
    async fn test_chapter_versioning() {
        let temp_dir = TempDir::new().unwrap();
        let root_path = temp_dir.path().to_path_buf();

        let mut project = NovelProject::new(root_path.clone(), "Test Novel".to_string());
        project.initialize().await.unwrap();

        let chapter_id = project.create_chapter("Test Chapter".to_string(), None).await.unwrap();

        // Update content multiple times
        project.update_chapter_content(chapter_id, "Content v1".to_string(), None).await.unwrap();
        project.update_chapter_content(chapter_id, "Content v2".to_string(), None).await.unwrap();
        project.update_chapter_content(chapter_id, "Content v3".to_string(), None).await.unwrap();

        // Check version history
        let history = project.get_version_history(chapter_id).await.unwrap();
        assert_eq!(history.len(), 2); // v1 and v2, not v3 (current)

        // Check current version
        let chapter = project.chapters.get(&chapter_id).unwrap();
        assert_eq!(chapter.current_version, 3);
        assert_eq!(chapter.content, "Content v3");
    }

    #[tokio::test]
    async fn test_volume_operations() {
        let temp_dir = TempDir::new().unwrap();
        let root_path = temp_dir.path().to_path_buf();

        let mut project = NovelProject::new(root_path.clone(), "Test Novel".to_string());
        project.initialize().await.unwrap();

        // Create a new volume
        let volume_id = project.create_volume("Volume 2".to_string()).await.unwrap();
        assert_eq!(project.volumes.len(), 2);

        // Create chapters in different volumes
        let chapter1_id = project.create_chapter("Chapter 1".to_string(), None).await.unwrap();
        let chapter2_id = project.create_chapter("Chapter 2".to_string(), Some(volume_id)).await.unwrap();

        let chapter1 = project.chapters.get(&chapter1_id).unwrap();
        let chapter2 = project.chapters.get(&chapter2_id).unwrap();

        assert_ne!(chapter1.volume_id, chapter2.volume_id);
    }

    #[tokio::test]
    async fn test_chapter_reorder() {
        let temp_dir = TempDir::new().unwrap();
        let root_path = temp_dir.path().to_path_buf();

        let mut project = NovelProject::new(root_path.clone(), "Test Novel".to_string());
        project.initialize().await.unwrap();

        let chapter1_id = project.create_chapter("Chapter 1".to_string(), None).await.unwrap();
        let chapter2_id = project.create_chapter("Chapter 2".to_string(), None).await.unwrap();
        let chapter3_id = project.create_chapter("Chapter 3".to_string(), None).await.unwrap();

        let default_volume_id = project.volumes[0].id;

        // Reorder: 3, 1, 2
        project.reorder_chapters_in_volume(
            default_volume_id,
            vec![chapter3_id, chapter1_id, chapter2_id],
        ).await.unwrap();

        let chapters = project.get_all_chapters_in_order();
        assert_eq!(chapters[0].id, chapter3_id);
        assert_eq!(chapters[1].id, chapter1_id);
        assert_eq!(chapters[2].id, chapter2_id);
    }
}
