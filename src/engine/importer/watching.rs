use std::{
    path::Path,
    sync::{mpsc::channel, Arc, Mutex},
    thread::{self, JoinHandle},
    time::Duration,
};

use notify::{watcher, DebouncedEvent, INotifyWatcher, RecursiveMode, Watcher};
use thiserror::Error;

use crate::{
    codegen::Module,
    util::{error_printing::PrintableError, path::path_relative_to_current_dir},
};

use super::{
    file_type::FileType,
    importer::Importer,
    module_collection::{ModuleCollection, ModuleCollectionError},
};

#[derive(Debug, Clone)]
pub struct WatchingImporter(Arc<WatchingInternals>);

impl Importer for WatchingImporter {
    fn get_module_from_endpoint(&self, endpoint: &str) -> anyhow::Result<Arc<Module>> {
        self.0
            .collection
            .lock()
            .map_err(|_| anyhow!("failed to unlock"))?
            .endpoints
            .get(endpoint)
            .cloned()
            .ok_or_else(|| anyhow!("module does not exist"))
    }

    fn get_module_from_location(&self, location: &Path) -> anyhow::Result<Arc<Module>> {
        self.0
            .collection
            .lock()
            .map_err(|_| anyhow!("failed to unlock"))?
            .locations
            .get(location)
            .cloned()
            .ok_or_else(|| anyhow!("module does not exist"))
    }

    fn get_all_endpoints(&self) -> anyhow::Result<Vec<String>> {
        let res = self
            .0
            .collection
            .lock()
            .map_err(|_| anyhow!("failed to unlock"))?
            .endpoints
            .keys()
            .cloned()
            .collect();
        Ok(res)
    }
}

impl WatchingImporter {
    pub fn new(directory: &str, extension: &str) -> anyhow::Result<Self> {
        let internals = WatchingInternals::new(directory, extension)?;
        Ok(Self(Arc::new(internals)))
    }
}

#[derive(Debug, Error)]
pub enum EventError {
    #[error(transparent)]
    IOError(#[from] std::io::Error),
    #[error("Abort due to {0}")]
    AbortError(&'static str),
    #[error("{0}")]
    ModuleCollectionError(String, ModuleCollectionError),
    #[error("{0}")]
    NotifyError(#[from] notify::Error),
}

#[derive(Debug)]
struct WatchingInternals {
    collection: Arc<Mutex<ModuleCollection>>,
    handle: JoinHandle<()>,
}

impl WatchingInternals {
    pub fn new(directory: &str, extension: &str) -> anyhow::Result<Self> {
        let (collection, errors) = ModuleCollection::from_directory(directory, extension, false)?;

        let mut buf = String::new();

        if errors.len() != 0 {
            let plural = if errors.len() > 1 { "s" } else { "" };
            eprintln!("detected errors in the following file{}:\n", plural);
            for (path, err) in errors {
                match err.print_error(&mut buf, path.as_str()) {
                    Ok(_) => {
                        eprintln!("{}", buf);
                        buf.clear();
                    }
                    Err(err) => {
                        Err(err)?;
                    }
                }
            }
        }

        let collection = Arc::new(Mutex::new(collection));
        let handle = Self::create_watcher(collection.clone(), directory, extension)?;
        Ok(Self { collection, handle })
    }

    fn create_watcher(
        collection: Arc<Mutex<ModuleCollection>>,
        directory: &str,
        extension: &str,
    ) -> anyhow::Result<JoinHandle<()>> {
        // Create a channel to receive the events.
        let (tx, rx) = channel();

        // Create a watcher object, delivering debounced events.
        // The notification back-end is selected based on the platform.
        let mut watcher = watcher(tx, Duration::from_millis(250))?;

        // Add a path to be watched. All files and directories at that path and
        // below will be monitored for changes.
        watcher.watch(directory, RecursiveMode::Recursive)?;

        let ext = extension.to_owned();
        let mut buf = String::new();

        let handle = thread::spawn(move || loop {
            let event = rx
                .recv()
                .unwrap_or_else(|err| panic!("watch error: {:?}", err));

            if let Err(err) = listen_event(collection.as_ref(), &mut watcher, event, ext.as_str()) {
                match err {
                    EventError::ModuleCollectionError(path, err) => {
                        match err.print_error(&mut buf, path.as_str()) {
                            Ok(_) => {
                                error!("could not apply change due to error:\n\n{}", buf);
                                buf.clear();
                            }
                            Err(err) => warn!(
                                "could not display error when reporting issues in {}: {}",
                                path.as_str(),
                                err
                            ),
                        }
                    }
                    _ => warn!("failure while watching files {}", err),
                }
            }
        });

        Ok(handle)
    }
}

fn listen_event(
    collection: &Mutex<ModuleCollection>,
    watcher: &mut INotifyWatcher,
    evt: DebouncedEvent,
    ext: &str,
) -> Result<(), EventError> {
    let mutex_lock_error = "mutex lock failed";

    match evt {
        DebouncedEvent::Error(err, _) => Err(err)?,

        // Do nothing for these types
        DebouncedEvent::Rescan
        | DebouncedEvent::NoticeWrite(_)
        | DebouncedEvent::NoticeRemove(_) => {}

        // Remove
        DebouncedEvent::Remove(path) => match FileType::from(path.as_ref(), ext) {
            FileType::RightExtFile => {
                let mut guard = collection
                    .lock()
                    .map_err(|_| EventError::AbortError(mutex_lock_error))?;
                if guard.remove(path.as_ref()).ok() == Some(true) {
                    let path = path_relative_to_current_dir(path);
                    info!("noticed deletion of {}", path.to_string_lossy())
                }
            }
            _ => {}
        },

        // Rename
        DebouncedEvent::Rename(old, new) => match FileType::from(new.as_ref(), ext) {
            FileType::RightExtFile => {
                let mut guard = collection
                    .lock()
                    .map_err(|_| EventError::AbortError(mutex_lock_error))?;

                guard.transaction::<(), EventError, _>(|collection| {
                    let file_name = new.to_string_lossy().to_string();

                    if let Err(err) = collection.remove(old.as_ref()) {
                        return Err(EventError::ModuleCollectionError(file_name, err));
                    }
                    if let Err(err) = collection.upsert(new.clone()) {
                        Err(EventError::ModuleCollectionError(file_name, err))?
                    } else {
                        let new = path_relative_to_current_dir(new);
                        let old = path_relative_to_current_dir(old);
                        info!(
                            "noticed move from {} to {}",
                            old.to_string_lossy(),
                            new.to_string_lossy()
                        );
                        Ok(())
                    }
                })?;
            }
            _ => {}
        },

        // Write
        DebouncedEvent::Write(path) => match FileType::from(path.as_ref(), ext) {
            FileType::Dir => {
                watcher.watch(path, RecursiveMode::Recursive)?;
            }
            FileType::RightExtFile => {
                let mut guard = collection
                    .lock()
                    .map_err(|_| EventError::AbortError(mutex_lock_error))?;
                let file_name = path.to_string_lossy().to_string();
                match guard.upsert(path.clone()) {
                    Err(err) => Err(EventError::ModuleCollectionError(file_name, err))?,
                    Ok(_) => {
                        let path = path_relative_to_current_dir(path);
                        info!("noticed change in {}", path.to_string_lossy());
                    }
                }
            }
            _ => {}
        },

        // Upsert
        DebouncedEvent::Chmod(path) | DebouncedEvent::Create(path) => {
            match FileType::from(path.as_ref(), ext) {
                FileType::RightExtFile => {
                    let mut guard = collection
                        .lock()
                        .map_err(|_| EventError::AbortError(mutex_lock_error))?;
                    let file_name = path.to_string_lossy().to_string();
                    match guard.upsert(path.clone()) {
                        Err(err) => Err(EventError::ModuleCollectionError(file_name, err))?,
                        Ok(_) => {
                            let path = path_relative_to_current_dir(path);
                            info!("noticed change in {}", path.to_string_lossy());
                        }
                    }
                }
                _ => {}
            }
        }
    }

    Ok(())
}
