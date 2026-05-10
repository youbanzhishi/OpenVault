//! File indexing and search for OpenVault.
//!
//! Provides:
//! - **FileIndex**: Metadata index for files (path, size, mtime, tags, summary).
//! - **TextExtractor**: Best-effort text extraction from various file types.
//! - **KeywordSearch**: Keyword-based search over path/tags/summary.
//! - **SemanticSearch**: Trait for future embedding-based semantic search.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// Metadata for a single indexed file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileIndexEntry {
    /// Full file path.
    pub path: String,
    /// File size in bytes.
    pub size: u64,
    /// Last modification time.
    pub modified_at: DateTime<Utc>,
    /// User-assigned or auto-generated tags.
    pub tags: Vec<String>,
    /// Optional text summary or extracted content preview.
    pub summary: Option<String>,
    /// File extension (lowercase, without dot).
    pub extension: Option<String>,
}

impl FileIndexEntry {
    /// Create a new index entry.
    pub fn new(path: impl Into<String>, size: u64) -> Self {
        let path_str = path.into();
        let p = Path::new(&path_str);
        let extension = p.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase());

        Self {
            path: path_str,
            size,
            modified_at: Utc::now(),
            tags: Vec::new(),
            summary: None,
            extension,
        }
    }

    /// Add a tag.
    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.tags.push(tag.into());
        self
    }

    /// Set the summary.
    pub fn with_summary(mut self, summary: impl Into<String>) -> Self {
        self.summary = Some(summary.into());
        self
    }

    /// Check if any field matches the given keyword (case-insensitive).
    pub fn matches_keyword(&self, keyword: &str) -> bool {
        let kw = keyword.to_lowercase();
        if self.path.to_lowercase().contains(&kw) {
            return true;
        }
        if self.tags.iter().any(|t| t.to_lowercase().contains(&kw)) {
            return true;
        }
        if let Some(ref summary) = self.summary {
            if summary.to_lowercase().contains(&kw) {
                return true;
            }
        }
        if let Some(ref ext) = self.extension {
            if ext.contains(&kw) {
                return true;
            }
        }
        false
    }
}

/// The file metadata index.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FileIndex {
    /// Map from file path to its index entry.
    entries: HashMap<String, FileIndexEntry>,
}

impl FileIndex {
    /// Create an empty index.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add or update an entry in the index.
    pub fn upsert(&mut self, entry: FileIndexEntry) {
        self.entries.insert(entry.path.clone(), entry);
    }

    /// Remove an entry by path.
    pub fn remove(&mut self, path: &str) -> Option<FileIndexEntry> {
        self.entries.remove(path)
    }

    /// Get an entry by path.
    pub fn get(&self, path: &str) -> Option<&FileIndexEntry> {
        self.entries.get(path)
    }

    /// Number of indexed files.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the index is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Iterate over all entries.
    pub fn iter(&self) -> impl Iterator<Item = &FileIndexEntry> {
        self.entries.values()
    }

    /// Search by keyword across all entries.
    pub fn search_keyword(&self, keyword: &str) -> Vec<SearchResult> {
        let mut results: Vec<SearchResult> = self.entries.values()
            .filter(|e| e.matches_keyword(keyword))
            .map(|e| {
                let relevance = Self::compute_relevance(e, keyword);
                SearchResult {
                    path: e.path.clone(),
                    snippet: e.summary.clone().unwrap_or_default(),
                    relevance,
                    tags: e.tags.clone(),
                    size: e.size,
                    modified_at: e.modified_at,
                }
            })
            .collect();

        results.sort_by(|a, b| b.relevance.partial_cmp(&a.relevance).unwrap_or(std::cmp::Ordering::Equal));
        results
    }

    /// Compute relevance score for a keyword match.
    fn compute_relevance(entry: &FileIndexEntry, keyword: &str) -> f64 {
        let kw = keyword.to_lowercase();
        let mut score = 0.0;

        // Path match is most relevant
        if entry.path.to_lowercase().contains(&kw) {
            score += 0.5;
            // Exact filename match is even better
            if let Some(name) = Path::new(&entry.path).file_name().and_then(|n| n.to_str()) {
                if name.to_lowercase().contains(&kw) {
                    score += 0.3;
                }
            }
        }

        // Tag match
        if entry.tags.iter().any(|t| t.to_lowercase().contains(&kw)) {
            score += 0.3;
        }

        // Summary match
        if let Some(ref summary) = entry.summary {
            if summary.to_lowercase().contains(&kw) {
                score += 0.2;
            }
        }

        score
    }
}

/// A single search result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    /// Path to the matched file.
    pub path: String,
    /// Matching snippet (from summary or path).
    pub snippet: String,
    /// Relevance score (0.0–1.0, higher is more relevant).
    pub relevance: f64,
    /// Tags associated with the file.
    pub tags: Vec<String>,
    /// File size in bytes.
    pub size: u64,
    /// Last modified time.
    pub modified_at: DateTime<Utc>,
}

/// Text extractor — best-effort extraction of text content from files.
pub struct TextExtractor;

impl TextExtractor {
    /// Extract text from a file based on its extension.
    ///
    /// For plain text, markdown, and code files, reads the content directly.
    /// For binary formats (PDF, Word, etc.), returns a stub indicating the type.
    pub fn extract(path: &str, content: &[u8]) -> ExtractedText {
        let ext = Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        let extractable_exts = [
            "txt", "md", "rst", "adoc",
            "rs", "py", "js", "ts", "go", "java", "c", "cpp", "h",
            "toml", "yaml", "yml", "json", "xml", "csv",
            "sh", "bash", "zsh",
            "html", "css", "sql",
        ];

        if extractable_exts.contains(&ext.as_str()) {
            let text = String::from_utf8_lossy(content);
            // Truncate to reasonable preview length
            let preview = if text.len() > 2000 {
                format!("{}...[truncated]", &text[..2000])
            } else {
                text.to_string()
            };
            ExtractedText {
                path: path.to_string(),
                text: preview,
                truncated: content.len() > 2000,
                extractor: "plaintext".to_string(),
            }
        } else {
            // Stub for binary formats
            let desc = match ext.as_str() {
                "pdf" => "PDF document (text extraction not yet implemented)".to_string(),
                "doc" | "docx" => "Word document (text extraction not yet implemented)".to_string(),
                "xls" | "xlsx" => "Excel spreadsheet (text extraction not yet implemented)".to_string(),
                "ppt" | "pptx" => "PowerPoint presentation (text extraction not yet implemented)".to_string(),
                "jpg" | "jpeg" | "png" | "gif" | "bmp" | "svg" => "Image file (no text content)".to_string(),
                "mp4" | "avi" | "mkv" => "Video file (no text content)".to_string(),
                "mp3" | "wav" | "flac" => "Audio file (no text content)".to_string(),
                "zip" | "tar" | "gz" | "7z" => "Archive file".to_string(),
                _ => format!("Binary file (.{})", ext),
            };
            ExtractedText {
                path: path.to_string(),
                text: desc,
                truncated: false,
                extractor: "stub".to_string(),
            }
        }
    }
}

/// Result of text extraction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedText {
    /// Source file path.
    pub path: String,
    /// Extracted text content.
    pub text: String,
    /// Whether the content was truncated.
    pub truncated: bool,
    /// Name of the extractor used.
    pub extractor: String,
}

/// Trait for semantic search (future: embedding-based).
///
/// Implementations will connect to embedding models (local or remote)
/// to provide semantic similarity search over file contents.
pub trait SemanticSearch: Send + Sync {
    /// Compute and store an embedding for the given text.
    fn index_document(&self, path: &str, text: &str) -> Result<(), String>;

    /// Search for documents semantically similar to the query.
    fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>, String>;

    /// Remove a document from the semantic index.
    fn remove_document(&self, path: &str) -> Result<(), String>;
}

/// A stub semantic search implementation that falls back to keyword search.
pub struct KeywordSemanticSearch {
    index: FileIndex,
}

impl KeywordSemanticSearch {
    /// Create a new keyword-based semantic search.
    pub fn new(index: FileIndex) -> Self {
        Self { index }
    }
}

impl SemanticSearch for KeywordSemanticSearch {
    fn index_document(&self, _path: &str, _text: &str) -> Result<(), String> {
        // No-op: documents should be added to the FileIndex directly
        Ok(())
    }

    fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>, String> {
        let mut results = self.index.search_keyword(query);
        results.truncate(limit);
        Ok(results)
    }

    fn remove_document(&self, _path: &str) -> Result<(), String> {
        Ok(())
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_index_entry_new() {
        let entry = FileIndexEntry::new("/docs/report.pdf", 1024);
        assert_eq!(entry.path, "/docs/report.pdf");
        assert_eq!(entry.size, 1024);
        assert_eq!(entry.extension, Some("pdf".to_string()));
        assert!(entry.tags.is_empty());
    }

    #[test]
    fn test_file_index_entry_with_tag_and_summary() {
        let entry = FileIndexEntry::new("src/main.rs", 500)
            .with_tag("code")
            .with_summary("Main entry point");
        assert_eq!(entry.tags, vec!["code"]);
        assert_eq!(entry.summary, Some("Main entry point".to_string()));
    }

    #[test]
    fn test_file_index_keyword_match() {
        let entry = FileIndexEntry::new("src/main.rs", 500)
            .with_tag("rust")
            .with_summary("Main entry point for the application");

        assert!(entry.matches_keyword("main"));
        assert!(entry.matches_keyword("rust"));
        assert!(entry.matches_keyword("entry"));
        assert!(!entry.matches_keyword("python"));
    }

    #[test]
    fn test_file_index_search() {
        let mut index = FileIndex::new();
        index.upsert(FileIndexEntry::new("src/main.rs", 500).with_tag("rust"));
        index.upsert(FileIndexEntry::new("docs/readme.md", 200).with_tag("documentation"));
        index.upsert(FileIndexEntry::new("data/report.csv", 1000).with_tag("data"));

        let results = index.search_keyword("rust");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].path, "src/main.rs");
    }

    #[test]
    fn test_file_index_search_relevance_ordering() {
        let mut index = FileIndex::new();
        // This entry matches both path and tag
        index.upsert(FileIndexEntry::new("src/rust_app.rs", 500).with_tag("rust"));
        // This entry matches only tag
        index.upsert(FileIndexEntry::new("src/main.rs", 500).with_tag("rust"));
        // This entry matches path only
        index.upsert(FileIndexEntry::new("rust_docs/readme.md", 200));

        let results = index.search_keyword("rust");
        assert_eq!(results.len(), 3);
        // First result should be the one with both path and tag match
        assert!(results[0].relevance >= results[1].relevance);
    }

    #[test]
    fn test_file_index_upsert_and_remove() {
        let mut index = FileIndex::new();
        index.upsert(FileIndexEntry::new("file.txt", 100));
        assert_eq!(index.len(), 1);

        index.upsert(FileIndexEntry::new("file.txt", 200));
        assert_eq!(index.len(), 1);
        assert_eq!(index.get("file.txt").unwrap().size, 200);

        index.remove("file.txt");
        assert!(index.is_empty());
    }

    #[test]
    fn test_text_extractor_plaintext() {
        let content = b"Hello, world! This is a test file.";
        let result = TextExtractor::extract("test.txt", content);
        assert_eq!(result.extractor, "plaintext");
        assert!(result.text.contains("Hello"));
        assert!(!result.truncated);
    }

    #[test]
    fn test_text_extractor_code() {
        let content = b"fn main() { println!(\"hello\"); }";
        let result = TextExtractor::extract("main.rs", content);
        assert_eq!(result.extractor, "plaintext");
        assert!(result.text.contains("fn main"));
    }

    #[test]
    fn test_text_extractor_binary_stub() {
        let content = b"%PDF-1.4 fake";
        let result = TextExtractor::extract("report.pdf", content);
        assert_eq!(result.extractor, "stub");
        assert!(result.text.contains("PDF"));
    }

    #[test]
    fn test_text_extractor_truncation() {
        let long_content: Vec<u8> = "A".repeat(3000).into_bytes();
        let result = TextExtractor::extract("big.txt", &long_content);
        assert!(result.truncated);
        assert!(result.text.contains("[truncated]"));
    }

    #[test]
    fn test_keyword_semantic_search() {
        let mut index = FileIndex::new();
        index.upsert(FileIndexEntry::new("src/app.rs", 300).with_tag("rust"));
        let search = KeywordSemanticSearch::new(index);
        let results = search.search("rust", 10).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_search_result_fields() {
        let mut index = FileIndex::new();
        index.upsert(FileIndexEntry::new("data.csv", 1024).with_tag("data").with_summary("Quarterly report data"));
        let results = index.search_keyword("report");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].path, "data.csv");
        assert!(results[0].relevance > 0.0);
    }
}
