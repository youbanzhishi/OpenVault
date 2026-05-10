//! File importance classifier.
//!
//! Classifies files by path/name/extension patterns into categories and
//! assigns backup priorities. Supports custom regex-based rules that
//! override the built-in heuristics.

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Backup priority level assigned to classified files.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackupPriority {
    /// Do not back up (temp files, caches, etc.)
    None = 0,
    /// Low priority — back up when convenient.
    Low = 1,
    /// Medium priority — normal scheduled backup.
    #[default]
    Medium = 2,
    /// High priority — real-time or near-real-time backup.
    High = 3,
}

/// Semantic file category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileCategory {
    /// Source code files.
    Code,
    /// Configuration files.
    Config,
    /// Documents (PDF, Word, etc.).
    Document,
    /// Images and photos.
    Image,
    /// Video files.
    Video,
    /// Audio files.
    Audio,
    /// Database or data files.
    Data,
    /// Temporary or cache files.
    Temp,
    /// Log files.
    Log,
    /// Everything else.
    Other,
}

impl FileCategory {
    /// Default backup priority for each category.
    pub fn default_priority(&self) -> BackupPriority {
        match self {
            FileCategory::Code => BackupPriority::High,
            FileCategory::Config => BackupPriority::High,
            FileCategory::Document => BackupPriority::Medium,
            FileCategory::Image => BackupPriority::High,
            FileCategory::Video => BackupPriority::Low,
            FileCategory::Audio => BackupPriority::Low,
            FileCategory::Data => BackupPriority::High,
            FileCategory::Temp => BackupPriority::None,
            FileCategory::Log => BackupPriority::Low,
            FileCategory::Other => BackupPriority::Medium,
        }
    }

    /// Suggested backup mode for each category.
    pub fn suggested_backup_mode(&self) -> &'static str {
        match self {
            FileCategory::Code => "realtime",
            FileCategory::Config => "realtime",
            FileCategory::Document => "scheduled",
            FileCategory::Image => "scheduled",
            FileCategory::Video => "scheduled",
            FileCategory::Audio => "scheduled",
            FileCategory::Data => "realtime",
            FileCategory::Temp => "none",
            FileCategory::Log => "scheduled",
            FileCategory::Other => "scheduled",
        }
    }
}

/// A custom classification rule that uses regex to match file paths.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassificationRule {
    /// Unique name for this rule.
    pub name: String,
    /// Regex pattern applied against the full file path.
    pub pattern: String,
    /// Category to assign when the pattern matches.
    pub category: FileCategory,
    /// Priority override when the pattern matches.
    pub priority: BackupPriority,
    /// Whether this rule is enabled.
    pub enabled: bool,
}

impl ClassificationRule {
    /// Create a new classification rule.
    pub fn new(
        name: impl Into<String>,
        pattern: impl Into<String>,
        category: FileCategory,
        priority: BackupPriority,
    ) -> Self {
        Self {
            name: name.into(),
            pattern: pattern.into(),
            category,
            priority,
            enabled: true,
        }
    }

    /// Compile the regex pattern.
    pub fn compile(&self) -> Result<Regex, regex::Error> {
        Regex::new(&self.pattern)
    }
}

/// Result of classifying a single file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileClassification {
    /// The file path that was classified.
    pub path: String,
    /// Assigned category.
    pub category: FileCategory,
    /// Assigned backup priority.
    pub priority: BackupPriority,
    /// Suggested backup mode.
    pub backup_mode: String,
    /// Name of the rule that matched (if a custom rule matched).
    pub matched_rule: Option<String>,
}

/// The file classifier engine.
pub struct FileClassifier {
    /// Custom rules (evaluated before built-in heuristics).
    custom_rules: Vec<ClassificationRule>,
}

impl FileClassifier {
    /// Create a new classifier with no custom rules.
    pub fn new() -> Self {
        Self {
            custom_rules: Vec::new(),
        }
    }

    /// Create a classifier with custom rules.
    pub fn with_rules(rules: Vec<ClassificationRule>) -> Self {
        Self {
            custom_rules: rules,
        }
    }

    /// Add a custom rule.
    pub fn add_rule(&mut self, rule: ClassificationRule) {
        self.custom_rules.push(rule);
    }

    /// Remove a custom rule by name.
    pub fn remove_rule(&mut self, name: &str) -> bool {
        let before = self.custom_rules.len();
        self.custom_rules.retain(|r| r.name != name);
        self.custom_rules.len() < before
    }

    /// Classify a single file path.
    pub fn classify(&self, path: &str) -> FileClassification {
        // 1. Try custom rules first
        for rule in &self.custom_rules {
            if !rule.enabled {
                continue;
            }
            if let Ok(re) = rule.compile() {
                if re.is_match(path) {
                    return FileClassification {
                        path: path.to_string(),
                        category: rule.category,
                        priority: rule.priority,
                        backup_mode: rule.category.suggested_backup_mode().to_string(),
                        matched_rule: Some(rule.name.clone()),
                    };
                }
            }
        }

        // 2. Built-in heuristics
        let category = Self::builtin_category(path);
        let priority = category.default_priority();

        FileClassification {
            path: path.to_string(),
            category,
            priority,
            backup_mode: category.suggested_backup_mode().to_string(),
            matched_rule: None,
        }
    }

    /// Classify multiple file paths.
    pub fn classify_batch(&self, paths: &[&str]) -> Vec<FileClassification> {
        paths.iter().map(|p| self.classify(p)).collect()
    }

    /// Determine file category using built-in heuristics.
    fn builtin_category(path: &str) -> FileCategory {
        let p = Path::new(path);
        let file_name = p.file_name().and_then(|n| n.to_str()).unwrap_or("").to_lowercase();
        let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
        let path_lower = path.to_lowercase();

        // Temp/cache directories
        if path_lower.contains("/tmp/")
            || path_lower.contains("/temp/")
            || path_lower.contains("/cache/")
            || path_lower.contains("/__pycache__/")
            || path_lower.contains("/.cache/")
            || path_lower.contains("\\tmp\\")
            || path_lower.contains("\\temp\\")
            || path_lower.contains("\\cache\\")
            || path_lower.starts_with("tmp/")
            || path_lower.starts_with("temp/")
            || path_lower.starts_with("cache/")
            || path_lower.starts_with("__pycache__/")
            || path_lower.starts_with(".cache/")
        {
            return FileCategory::Temp;
        }

        // Node modules, build artifacts → temp (check before extension-based categories)
        if path_lower.contains("/node_modules/")
            || path_lower.contains("/target/debug/")
            || path_lower.contains("/build/")
            || path_lower.contains("/dist/")
        {
            return FileCategory::Temp;
        }

        // Temp file extensions
        if ext == "tmp" || ext == "temp" || ext == "bak" || ext == "swp" || ext == "swo" {
            return FileCategory::Temp;
        }

        // Log files
        if ext == "log" || file_name.starts_with("log.") || file_name.contains(".log.") {
            return FileCategory::Log;
        }

        // Code files
        let code_exts = [
            "rs", "py", "js", "ts", "jsx", "tsx", "go", "java", "c", "cpp", "h", "hpp",
            "rb", "php", "swift", "kt", "scala", "sh", "bash", "zsh", "fish",
            "toml", "yaml", "yml", "json", "xml", "cmake", "makefile", "dockerfile",
        ];
        if code_exts.contains(&ext.as_str()) {
            return FileCategory::Code;
        }

        // Config files (dotfiles, etc.)
        if file_name.starts_with('.') && !file_name.contains('.') || ext == "conf" || ext == "cfg" || ext == "ini" {
            return FileCategory::Config;
        }

        // Image files
        let image_exts = ["jpg", "jpeg", "png", "gif", "bmp", "svg", "webp", "tiff", "ico", "raw", "cr2", "nef"];
        if image_exts.contains(&ext.as_str()) {
            return FileCategory::Image;
        }

        // Video files
        let video_exts = ["mp4", "avi", "mkv", "mov", "wmv", "flv", "webm", "m4v"];
        if video_exts.contains(&ext.as_str()) {
            return FileCategory::Video;
        }

        // Audio files
        let audio_exts = ["mp3", "wav", "flac", "aac", "ogg", "wma", "m4a"];
        if audio_exts.contains(&ext.as_str()) {
            return FileCategory::Audio;
        }

        // Document files
        let doc_exts = ["pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "odt", "rtf", "tex", "md"];
        if doc_exts.contains(&ext.as_str()) {
            return FileCategory::Document;
        }

        // Data files
        let data_exts = ["db", "sqlite", "sql", "csv", "tsv", "parquet", "hdf5"];
        if data_exts.contains(&ext.as_str()) {
            return FileCategory::Data;
        }

        FileCategory::Other
    }
}

impl Default for FileClassifier {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_code_file() {
        let classifier = FileClassifier::new();
        let result = classifier.classify("src/main.rs");
        assert_eq!(result.category, FileCategory::Code);
        assert_eq!(result.priority, BackupPriority::High);
    }

    #[test]
    fn test_classify_image_file() {
        let classifier = FileClassifier::new();
        let result = classifier.classify("photos/vacation/beach.jpg");
        assert_eq!(result.category, FileCategory::Image);
        assert_eq!(result.priority, BackupPriority::High);
    }

    #[test]
    fn test_classify_temp_file() {
        let classifier = FileClassifier::new();
        let result = classifier.classify("/tmp/scratch.tmp");
        assert_eq!(result.category, FileCategory::Temp);
        assert_eq!(result.priority, BackupPriority::None);
    }

    #[test]
    fn test_classify_document_file() {
        let classifier = FileClassifier::new();
        let result = classifier.classify("reports/2024/Q3_report.pdf");
        assert_eq!(result.category, FileCategory::Document);
        assert_eq!(result.priority, BackupPriority::Medium);
    }

    #[test]
    fn test_classify_video_file() {
        let classifier = FileClassifier::new();
        let result = classifier.classify("videos/clip.mp4");
        assert_eq!(result.category, FileCategory::Video);
        assert_eq!(result.priority, BackupPriority::Low);
    }

    #[test]
    fn test_custom_rule_overrides_builtin() {
        let rules = vec![ClassificationRule::new(
            "important_logs",
            r".*important.*\.log$",
            FileCategory::Data,
            BackupPriority::High,
        )];
        let classifier = FileClassifier::with_rules(rules);
        let result = classifier.classify("data/important_audit.log");
        assert_eq!(result.category, FileCategory::Data);
        assert_eq!(result.priority, BackupPriority::High);
        assert_eq!(result.matched_rule, Some("important_logs".to_string()));
    }

    #[test]
    fn test_classify_batch() {
        let classifier = FileClassifier::new();
        let paths = vec!["main.py", "photo.png", "cache/tmp.dat"];
        let results = classifier.classify_batch(&paths);
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].category, FileCategory::Code);
        assert_eq!(results[1].category, FileCategory::Image);
        assert_eq!(results[2].category, FileCategory::Temp);
    }

    #[test]
    fn test_add_remove_rule() {
        let mut classifier = FileClassifier::new();
        classifier.add_rule(ClassificationRule::new(
            "test_rule",
            r".*\.xyz$",
            FileCategory::Data,
            BackupPriority::High,
        ));
        assert_eq!(classifier.custom_rules.len(), 1);
        let result = classifier.classify("data.xyz");
        assert_eq!(result.category, FileCategory::Data);

        assert!(classifier.remove_rule("test_rule"));
        assert!(classifier.custom_rules.is_empty());
        let result2 = classifier.classify("data.xyz");
        assert_eq!(result2.category, FileCategory::Other);
    }

    #[test]
    fn test_priority_ordering() {
        assert!(BackupPriority::High > BackupPriority::Medium);
        assert!(BackupPriority::Medium > BackupPriority::Low);
        assert!(BackupPriority::Low > BackupPriority::None);
    }

    #[test]
    fn test_log_file_classification() {
        let classifier = FileClassifier::new();
        let result = classifier.classify("var/log/syslog.log");
        assert_eq!(result.category, FileCategory::Log);
        assert_eq!(result.priority, BackupPriority::Low);
    }

    #[test]
    fn test_node_modules_is_temp() {
        let classifier = FileClassifier::new();
        let result = classifier.classify("project/node_modules/react/index.js");
        assert_eq!(result.category, FileCategory::Temp);
        assert_eq!(result.priority, BackupPriority::None);
    }

    #[test]
    fn test_config_file() {
        let classifier = FileClassifier::new();
        let result = classifier.classify("app.conf");
        assert_eq!(result.category, FileCategory::Config);
        assert_eq!(result.priority, BackupPriority::High);
    }

    #[test]
    fn test_disabled_rule_is_skipped() {
        let mut rule = ClassificationRule::new(
            "disabled_rule",
            r".*\.rs$",
            FileCategory::Data,
            BackupPriority::None,
        );
        rule.enabled = false;
        let classifier = FileClassifier::with_rules(vec![rule]);
        let result = classifier.classify("main.rs");
        // Should fall through to built-in heuristic
        assert_eq!(result.category, FileCategory::Code);
        assert!(result.matched_rule.is_none());
    }
}
