use std::{fs::Metadata, path::Path};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType {
    RightExtFile,
    OtherFile,
    Dir,
    Other,
}

impl FileType {
    pub fn from_metadata(path: &Path, metadata: &Metadata, ext: &str) -> Self {
        if metadata.is_dir() {
            FileType::Dir
        } else if metadata.is_file() {
            if path.extension().and_then(|ext| ext.to_str()) == Some(ext) {
                FileType::RightExtFile
            } else {
                FileType::OtherFile
            }
        } else {
            FileType::Other
        }
    }

    pub fn from(path: &Path, ext: &str) -> Self {
        if let Ok(metadata) = path.metadata() {
            Self::from_metadata(path, &metadata, ext)
        } else {
            FileType::Other
        }
    }
}
