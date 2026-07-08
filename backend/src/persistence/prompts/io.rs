use crate::errors::{AppError, AppResult};
use crate::persistence::prompts::schema::PromptFile;
use std::path::Path;

pub const EXT: &str = "quantamind.yaml";

pub fn read(path: &Path) -> AppResult<PromptFile> {
    if !path.exists() {
        return Err(AppError::NotFound(path.display().to_string()));
    }
    let content = std::fs::read_to_string(path).map_err(|e| AppError::Io(e.to_string()))?;
    serde_yaml::from_str(&content).map_err(|e| AppError::Internal(e.to_string()))
}

pub fn write(path: &Path, file: &PromptFile) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::Io(e.to_string()))?;
    }
    let yaml = serde_yaml::to_string(file).map_err(|e| AppError::Internal(e.to_string()))?;
    std::fs::write(path, yaml).map_err(|e| AppError::Io(e.to_string()))
}

pub fn delete(path: &Path) -> AppResult<()> {
    if path.is_dir() {
        std::fs::remove_dir_all(path).map_err(|e| AppError::Io(e.to_string()))
    } else if path.exists() {
        std::fs::remove_file(path).map_err(|e| AppError::Io(e.to_string()))
    } else {
        Err(AppError::NotFound(path.display().to_string()))
    }
}

pub fn rename(old: &Path, new: &Path) -> AppResult<()> {
    if !old.exists() {
        return Err(AppError::NotFound(old.display().to_string()));
    }
    if new.exists() {
        return Err(AppError::Validation(format!("already exists: {}", new.display())));
    }
    if let Some(parent) = new.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::Io(e.to_string()))?;
    }
    std::fs::rename(old, new).map_err(|e| AppError::Io(e.to_string()))
}

#[cfg(test)]
#[path = "io_tests.rs"]
mod tests;
