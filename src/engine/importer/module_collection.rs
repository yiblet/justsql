use crate::{
    codegen::{Module, ModuleError},
    util::{
        error_printing::{print_unpositioned_error, PrintableError},
        path::path_relative_to_current_dir,
    },
};
use std::{
    borrow::Borrow,
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};
use thiserror::Error;

use super::file_type::FileType;

#[derive(Error, Debug)]
pub enum ModuleCollectionError {
    #[error("failed importing module: {0}")]
    IOError(PathBuf, std::io::Error),
    #[error("path is not absolute")]
    NotAbsolutePath(PathBuf),
    #[error("module not found")]
    ModuleNotFound(PathBuf),
    #[error("endpoint {0} already in use")]
    AlreadyUsedEndpointError(PathBuf, String),
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
    ) -> Result<(), crate::util::error_printing::PrintError> {
        match self {
            ModuleCollectionError::IOError(path, _)
            | ModuleCollectionError::NotAbsolutePath(path)
            | ModuleCollectionError::AlreadyUsedEndpointError(path, _)
            | ModuleCollectionError::ModuleNotFound(path) => {
                // FIXME change relative pathing to current dir
                let path = path_relative_to_current_dir(path.as_path());
                let lossy = path.to_string_lossy();
                let file_name = lossy.as_ref(); // FIXME module errors must now contain the module they pointed to
                print_unpositioned_error(writer, self.to_string().as_ref(), file_name)?
            }
            ModuleCollectionError::ModuleError(err) => err.print_error(writer)?,
        };

        Ok(())
    }
}

impl ModuleCollection {
    pub fn from_directory(
        directory: &str,
        extension: &str,
        follow_links: bool,
    ) -> (Self, Vec<ModuleCollectionError>) {
        let mut errors: Vec<ModuleCollectionError> = vec![];
        let mut collection: ModuleCollection = Default::default();

        let path_bufs = walkdir::WalkDir::new(directory)
            .follow_links(follow_links)
            .into_iter()
            .filter_map(|entry| entry.ok())
            .filter_map(|entry| {
                if FileType::from(entry.path(), extension) == FileType::RightExtFile {
                    entry
                        .path()
                        .canonicalize()
                        .map_err(|err| {
                            ModuleCollectionError::IOError(entry.path().to_path_buf(), err)
                        })
                        .map_or_else(
                            |err| {
                                errors.push(err);
                                None
                            },
                            Some,
                        )
                } else {
                    None
                }
            })
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();

        let paths: Vec<&Path> = path_bufs.iter().map(|p| p.borrow()).collect();

        let (modules, module_errors) = Module::from_paths::<Module>(paths.as_slice(), None);
        errors.extend(module_errors.into_iter().map(ModuleCollectionError::from));
        for (path, module) in modules {
            if let Err(err) = collection.insert(path.to_path_buf(), module) {
                errors.push(err)
            }
        }

        (collection, errors)
    }

    pub fn import_module(&self, path: &Path) -> (BTreeMap<PathBuf, Module>, Vec<ModuleError>) {
        self.import_modules(&[path])
    }

    pub fn import_modules(&self, paths: &[&Path]) -> (BTreeMap<PathBuf, Module>, Vec<ModuleError>) {
        let deps = self
            .locations
            .iter()
            .map(|(path_buf, module)| (path_buf.as_path(), module.as_ref()))
            .collect();
        Module::from_paths(paths, Some(&deps))
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

    pub fn insert(
        &mut self,
        location: PathBuf,
        module: Module,
    ) -> Result<(), ModuleCollectionError> {
        self.transaction(|collection| {
            if !location.is_absolute() {
                return Err(ModuleCollectionError::NotAbsolutePath(location));
            }

            let module = Arc::new(module);

            // insert module endpoint
            if let Some(endpoint) = module.front_matter.endpoint.as_ref() {
                if collection.endpoints.contains_key(endpoint) {
                    return Err(ModuleCollectionError::AlreadyUsedEndpointError(
                        location,
                        endpoint.to_owned(),
                    ));
                };
                collection
                    .endpoints
                    .insert(endpoint.to_owned(), module.clone());
            }

            collection.locations.insert(location, module);
            Ok(())
        })
    }

    pub fn upsert(
        &mut self,
        location: PathBuf,
        module: Module,
    ) -> Result<(), ModuleCollectionError> {
        self.transaction(|collection| {
            collection.remove(location.as_path())?;
            collection.insert(location, module)
        })
    }

    pub fn remove(&mut self, location: &Path) -> Result<bool, ModuleCollectionError> {
        let canonical;
        let new_loc; // need to create a second location variable to ensure lifetime is less than `location`
        if !location.is_absolute() {
            canonical = fs::canonicalize(location)
                .map_err(|_| ModuleCollectionError::ModuleNotFound(location.to_path_buf()))?;
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
