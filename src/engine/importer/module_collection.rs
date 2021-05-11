use crate::{
    codegen::{Module, ModuleError},
    util::{
        error_printing::{print_error, print_unpositioned_error, PrintableError},
        path::path_relative_to_current_dir,
    },
};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};
use thiserror::Error;

use super::file_type::FileType;

#[derive(Error, Debug)]
pub enum ModuleCollectionError {
    #[error("module not found")]
    ModuleNotFound,
    #[error("endpoint {0} already in use")]
    AlreadyUsedEndpointError(String),
    #[error("{0}")]
    ModuleError(#[from] ModuleError),
}

#[derive(Default, Clone, Debug)]
pub struct ModuleCollection {
    pub endpoints: im::OrdMap<String, Arc<Module>>,
    pub locations: im::OrdMap<PathBuf, Arc<Module>>,
}

impl PrintableError for ModuleCollectionError {
    fn print_error<W: std::fmt::Write>(
        &self,
        writer: &mut W,
        file_name: &str,
    ) -> Result<(), crate::util::error_printing::PrintError> {
        let path = path_relative_to_current_dir(Path::new(file_name).to_path_buf());
        let lossy = path.to_string_lossy();
        let file_name = lossy.as_ref();

        match self {
            ModuleCollectionError::AlreadyUsedEndpointError(_)
            | ModuleCollectionError::ModuleNotFound => {
                print_unpositioned_error(writer, self.to_string().as_ref(), file_name)?
            }
            ModuleCollectionError::ModuleError(err) => match err {
                ModuleError::IOError(_) | ModuleError::Incomplete => {
                    print_unpositioned_error(writer, err.to_string().as_ref(), file_name)?
                }
                ModuleError::MultipleParseError { file, errors } => {
                    for (pos, err) in errors.iter() {
                        print_error(writer, file.as_str(), *pos, err.as_str(), file_name)?
                    }
                }

                ModuleError::ParseError { file, pos, .. }
                | ModuleError::NomParseError { file, pos } => print_error(
                    writer,
                    file.as_str(),
                    *pos,
                    err.to_string().as_str(),
                    file_name,
                )?,
            },
        };

        Ok(())
    }
}

impl ModuleCollection {
    pub fn from_directory(
        directory: &str,
        extension: &str,
        follow_links: bool,
    ) -> std::io::Result<(Self, Vec<(String, ModuleCollectionError)>)> {
        let mut errors = vec![];
        let mut collection: ModuleCollection = Default::default();

        let directory = fs::canonicalize(directory)?;
        for entry in walkdir::WalkDir::new(directory)
            .follow_links(follow_links)
            .into_iter()
            .filter_map(|entry| entry.ok())
        {
            match FileType::from(entry.path(), extension) {
                FileType::RightExtFile => {
                    if let Err(err) = collection.insert(entry.path().to_owned()) {
                        errors.push((entry.path().to_string_lossy().to_string(), err));
                    }
                }
                _ => {}
            }
        }

        Ok((collection, errors))
    }

    // use immutable datastructures to make atomicity trivial
    // inserts and can be unwound by simple maintaining a reference to the unchanged
    // version of the mutations.

    // only applies all changes if the result is ok
    #[inline]
    pub fn transaction<O, E, F: FnOnce(&mut Self) -> Result<O, E>>(
        &mut self,
        func: F,
    ) -> Result<O, E> {
        let mut editable = self.clone();
        let res = func(&mut editable);

        if let Ok(_) = res {
            self.endpoints = editable.endpoints;
            self.locations = editable.locations;
        }
        res
    }

    pub fn insert(&mut self, mut location: PathBuf) -> Result<(), ModuleCollectionError> {
        self.transaction(|collection| {
            if !location.is_absolute() {
                location = fs::canonicalize(location)
                    .map_err(|_| ModuleCollectionError::ModuleNotFound)?;
            }

            let module = Arc::new(Module::from_path(&location)?);

            // insert module endpoint
            if let Some(endpoint) = module.front_matter.endpoint.as_ref() {
                if collection.endpoints.contains_key(endpoint) {
                    Err(ModuleCollectionError::AlreadyUsedEndpointError(
                        endpoint.to_owned(),
                    ))?
                };
                collection
                    .endpoints
                    .insert(endpoint.to_owned(), module.clone());
            }

            collection.locations.insert(location, module);
            Ok(())
        })
    }

    pub fn upsert(&mut self, location: PathBuf) -> Result<(), ModuleCollectionError> {
        self.transaction(|collection| {
            collection.remove(location.as_path())?;
            collection.insert(location)
        })
    }

    pub fn remove(&mut self, location: &Path) -> Result<bool, ModuleCollectionError> {
        let canonical;
        let new_loc; // need to create a second location variable to ensure lifetime is less than `location`
        if !location.is_absolute() {
            canonical =
                fs::canonicalize(location).map_err(|_| ModuleCollectionError::ModuleNotFound)?;
            new_loc = canonical.as_ref();
        } else {
            new_loc = location;
        }
        // no need for transactions since this cannot fail
        let removed_arc = self.locations.remove(new_loc);
        match removed_arc
            .as_ref()
            .and_then(|arc| arc.front_matter.endpoint.as_ref())
        {
            Some(endpoint) => {
                self.endpoints.remove(endpoint);
                Ok(true)
            }
            None => Ok(removed_arc.is_some()),
        }
    }
}
