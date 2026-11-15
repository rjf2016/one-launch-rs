use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use async_channel::{Receiver, Sender};
use ol_model::{Account, LibrarySort};
use ol_services::data_folder::{self, OpenError};
use ol_services::library::{self, LibraryGame};
use ol_store::{Store, StoreError};
use thiserror::Error;

// What the Electron app sends across to its page for a failure: a code to act on and a sentence to show.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
#[error("{message}")]
pub struct ApiError {
    pub code: &'static str,
    pub message: String,
}

impl ApiError {
    fn stopped() -> Self {
        Self {
            code: "INTERNAL",
            message: "One Launch is no longer reading its data. Restart One Launch.".to_owned(),
        }
    }
}

impl From<StoreError> for ApiError {
    fn from(error: StoreError) -> Self {
        Self {
            code: error.code(),
            message: error.to_string(),
        }
    }
}

impl From<OpenError> for ApiError {
    fn from(error: OpenError) -> Self {
        Self {
            code: error.code(),
            message: error.to_string(),
        }
    }
}

pub struct Library {
    pub games: Vec<LibraryGame>,
    pub sort: LibrarySort,
}

type Reply<T> = Sender<Result<T, ApiError>>;

enum Command {
    Library(Reply<Library>),
    Accounts(Reply<Vec<Account>>),
    SetLibrarySort(LibrarySort, Reply<()>),
    BackUpDatabase(Reply<()>),
}

// The database lives on one thread of its own. A handle only sends that thread a request and waits for the answer, so it can be held and cloned anywhere.
#[derive(Clone)]
pub struct Backend {
    commands: Sender<Command>,
    thread: Arc<Mutex<Option<JoinHandle<()>>>>,
}

impl Backend {
    // Returns at once: the folder is made and the database opened on the new thread, and a failure there is the answer to every request.
    pub fn start(data_dir: PathBuf, version: &str) -> Self {
        let (commands, requests) = async_channel::unbounded();
        let version = version.to_owned();
        let thread = thread::Builder::new()
            .name("backend".to_owned())
            .spawn(move || serve(&data_dir, &version, &requests))
            .ok();
        Self {
            commands,
            thread: Arc::new(Mutex::new(thread)),
        }
    }

    pub async fn library(&self) -> Result<Library, ApiError> {
        self.ask(Command::Library).await
    }

    pub async fn accounts(&self) -> Result<Vec<Account>, ApiError> {
        self.ask(Command::Accounts).await
    }

    pub async fn set_library_sort(&self, sort: LibrarySort) -> Result<(), ApiError> {
        self.ask(|reply| Command::SetLibrarySort(sort, reply)).await
    }

    pub async fn back_up_database(&self) -> Result<(), ApiError> {
        self.ask(Command::BackUpDatabase).await
    }

    // Waits for the request in hand and for the database to close, which folds its journal back into the one file.
    pub fn shut_down(&self) {
        self.commands.close();
        let thread = match self.thread.lock() {
            Ok(mut thread) => thread.take(),
            Err(poisoned) => poisoned.into_inner().take(),
        };
        if let Some(thread) = thread {
            let _ = thread.join();
        }
    }

    async fn ask<T>(&self, command: impl FnOnce(Reply<T>) -> Command) -> Result<T, ApiError> {
        let (reply, answer) = async_channel::bounded(1);
        self.commands
            .send(command(reply))
            .await
            .map_err(|_| ApiError::stopped())?;
        answer
            .recv()
            .await
            .unwrap_or_else(|_| Err(ApiError::stopped()))
    }
}

fn serve(data_dir: &Path, version: &str, requests: &Receiver<Command>) {
    let opened = data_folder::open(data_dir, version)
        .map(|(store, _)| store)
        .map_err(ApiError::from);
    while let Ok(command) = requests.recv_blocking() {
        let store = opened.as_ref().map_err(ApiError::clone);
        match command {
            Command::Library(reply) => answer(
                &reply,
                store.and_then(|store| read_library(store, data_dir)),
            ),
            Command::Accounts(reply) => {
                answer(&reply, store.and_then(|store| Ok(store.accounts()?)))
            }
            Command::SetLibrarySort(sort, reply) => answer(
                &reply,
                store.and_then(|store| Ok(store.set_library_sort(sort)?)),
            ),
            Command::BackUpDatabase(reply) => answer(
                &reply,
                store.and_then(|store| {
                    data_folder::back_up(store, data_dir)?;
                    Ok(())
                }),
            ),
        }
    }
}

fn read_library(store: &Store, data_dir: &Path) -> Result<Library, ApiError> {
    Ok(Library {
        games: library::list(store, data_dir)?,
        sort: store.library_sort()?,
    })
}

fn answer<T>(reply: &Reply<T>, result: Result<T, ApiError>) {
    // Whoever asked may have gone away meanwhile, and then nobody is left to tell.
    let _ = reply.send_blocking(result);
}
