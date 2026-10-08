// SPDX-License-Identifier: GPL-3.0

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrashItem {
    pub id: String,
    pub name: String,
    pub original_path: PathBuf,
    pub trash_file_path: PathBuf,
    pub trash_info_path: PathBuf,
    pub size_bytes: u64,
    pub deletion_date: String,
}

pub struct TrashBackend;

impl TrashBackend {
    pub fn get_trash_dir() -> PathBuf {
        if let Ok(xdg_data) = std::env::var("XDG_DATA_HOME") {
            PathBuf::from(xdg_data).join("Trash")
        } else if let Ok(home) = std::env::var("HOME") {
            PathBuf::from(home).join(".local/share/Trash")
        } else {
            PathBuf::from("~/.local/share/Trash")
        }
    }

    pub fn list_items() -> Vec<TrashItem> {
        let trash_dir = Self::get_trash_dir();
        let files_dir = trash_dir.join("files");
        let info_dir = trash_dir.join("info");

        let mut items = Vec::new();

        if let Ok(entries) = fs::read_dir(&files_dir) {
            for entry in entries.flatten() {
                let trash_file_path = entry.path();
                let file_name = match entry.file_name().into_string() {
                    Ok(n) => n,
                    Err(_) => continue,
                };

                let info_file_name = format!("{}.trashinfo", file_name);
                let trash_info_path = info_dir.join(&info_file_name);

                let mut original_path = PathBuf::from(&file_name);
                let mut deletion_date = String::new();

                if trash_info_path.exists() {
                    if let Ok(info_content) = fs::read_to_string(&trash_info_path) {
                        let (parsed_path, parsed_date) = Self::parse_trashinfo(&info_content);
                        if let Some(p) = parsed_path {
                            original_path = p;
                        }
                        if let Some(d) = parsed_date {
                            deletion_date = d;
                        }
                    }
                }

                let display_name = original_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(&file_name)
                    .to_string();

                let size_bytes = Self::compute_path_size(&trash_file_path);

                items.push(TrashItem {
                    id: file_name,
                    name: display_name,
                    original_path,
                    trash_file_path,
                    trash_info_path,
                    size_bytes,
                    deletion_date,
                });
            }
        }

        // Sort items by deletion date descending (newest first)
        items.sort_by(|a, b| b.deletion_date.cmp(&a.deletion_date));
        items
    }

    pub fn parse_trashinfo(content: &str) -> (Option<PathBuf>, Option<String>) {
        let mut original_path = None;
        let mut deletion_date = None;

        for line in content.lines() {
            let line = line.trim();
            if line.starts_with("Path=") {
                let raw_path = &line[5..];
                let decoded_path = urlencoding::decode(raw_path)
                    .unwrap_or_else(|_| raw_path.into());
                original_path = Some(PathBuf::from(decoded_path.as_ref()));
            } else if line.starts_with("DeletionDate=") {
                deletion_date = Some(line[13..].to_string());
            }
        }

        (original_path, deletion_date)
    }

    pub fn compute_path_size(path: &Path) -> u64 {
        if let Ok(metadata) = fs::symlink_metadata(path) {
            if metadata.is_dir() {
                let mut total = metadata.len();
                if let Ok(entries) = fs::read_dir(path) {
                    for entry in entries.flatten() {
                        total += Self::compute_path_size(&entry.path());
                    }
                }
                total
            } else {
                metadata.len()
            }
        } else {
            0
        }
    }

    pub fn calculate_total_size(items: &[TrashItem]) -> u64 {
        items.iter().map(|item| item.size_bytes).sum()
    }

    pub fn format_size(bytes: u64) -> String {
        const KB: u64 = 1024;
        const MB: u64 = KB * 1024;
        const GB: u64 = MB * 1024;

        if bytes >= GB {
            format!("{:.1} GB", bytes as f64 / GB as f64)
        } else if bytes >= MB {
            format!("{:.1} MB", bytes as f64 / MB as f64)
        } else if bytes >= KB {
            format!("{:.1} KB", bytes as f64 / KB as f64)
        } else {
            format!("{} B", bytes)
        }
    }

    pub fn empty_trash() -> Result<(), std::io::Error> {
        let trash_dir = Self::get_trash_dir();
        let files_dir = trash_dir.join("files");
        let info_dir = trash_dir.join("info");

        if let Ok(entries) = fs::read_dir(&files_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let _ = fs::remove_dir_all(&path);
                } else {
                    let _ = fs::remove_file(&path);
                }
            }
        }

        if let Ok(entries) = fs::read_dir(&info_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let _ = fs::remove_dir_all(&path);
                } else {
                    let _ = fs::remove_file(&path);
                }
            }
        }

        Ok(())
    }

    pub fn restore_item(item: &TrashItem) -> Result<(), std::io::Error> {
        if let Some(parent) = item.original_path.parent() {
            fs::create_dir_all(parent)?;
        }

        if item.trash_file_path.exists() {
            fs::rename(&item.trash_file_path, &item.original_path).or_else(|_| {
                if item.trash_file_path.is_dir() {
                    copy_dir_all(&item.trash_file_path, &item.original_path)?;
                    fs::remove_dir_all(&item.trash_file_path)
                } else {
                    fs::copy(&item.trash_file_path, &item.original_path)?;
                    fs::remove_file(&item.trash_file_path)
                }
            })?;
        }

        if item.trash_info_path.exists() {
            let _ = fs::remove_file(&item.trash_info_path);
        }

        Ok(())
    }

    pub fn open_trash_in_file_manager() {
        let trash_dir = Self::get_trash_dir().join("files");
        let _ = fs::create_dir_all(&trash_dir);

        // Try `gio open trash:///` first (native COSMIC/Freedesktop trash URI handler)
        if Command::new("gio")
            .arg("open")
            .arg("trash:///")
            .spawn()
            .is_err()
        {
            // Fallback to direct directory path in cosmic-files or xdg-open
            if Command::new("cosmic-files")
                .arg(&trash_dir)
                .spawn()
                .is_err()
            {
                let _ = Command::new("xdg-open")
                    .arg(&trash_dir)
                    .spawn();
            }
        }
    }
}

fn copy_dir_all(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> std::io::Result<()> {
    fs::create_dir_all(&dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_dir() {
            copy_dir_all(entry.path(), dst.as_ref().join(entry.file_name()))?;
        } else {
            fs::copy(entry.path(), dst.as_ref().join(entry.file_name()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_trashinfo() {
        let info = "[Trash Info]\nPath=/home/user/My%20Document.txt\nDeletionDate=2026-10-08T12:00:00\n";
        let (path, date) = TrashBackend::parse_trashinfo(info);

        assert_eq!(path, Some(PathBuf::from("/home/user/My Document.txt")));
        assert_eq!(date, Some("2026-10-08T12:00:00".to_string()));
    }

    #[test]
    fn test_format_size() {
        assert_eq!(TrashBackend::format_size(500), "500 B");
        assert_eq!(TrashBackend::format_size(2048), "2.0 KB");
        assert_eq!(TrashBackend::format_size(1048576), "1.0 MB");
        assert_eq!(TrashBackend::format_size(1073741824), "1.0 GB");
    }
}
