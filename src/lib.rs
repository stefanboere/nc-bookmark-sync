use serde::{Deserialize, Serialize};
use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::io::prelude::*;
use std::path::PathBuf;
use thiserror::Error;

type UnixTime = u64;

pub fn main() {
    let args: Vec<String> = env::args().collect();
    let configfile: PathBuf = if args.len() > 1 {
        let mut configdir = env::current_dir().unwrap();
        configdir.push(&args[1]);
        configdir
    } else {
        let mut configdir = dirs::config_dir().unwrap();
        configdir.push("nc-bookmark-sync/config.toml");
        configdir
    };

    if !configfile.exists() {
        panic!("Config file {0} does not exist", configfile.display());
    }

    let contents = fs::read_to_string(configfile).unwrap();
    let config: Config = toml::from_str(&contents).unwrap();

    for (name, pair) in config.pair.iter() {
        let storage_a = config
            .storage
            .get(&pair.a)
            .ok_or(Error::StorageNotFound("a"))
            .unwrap();
        let storage_b = config
            .storage
            .get(&pair.b)
            .ok_or(Error::StorageNotFound("b"))
            .unwrap();

        let state_file = config.general.status_path.clone() + "/" + &name;

        Pair::new(state_file, pair, storage_a, storage_b)
            .unwrap()
            .run()
            .unwrap();
    }
}

#[derive(Error, Debug)]
enum Error {
    #[error("Missing config entry `{0}`")]
    MissingConfig(&'static str),
    #[error("IO Error: {0}")]
    IOError(std::io::Error),
    #[error("UTF8 parse error: {0}")]
    Utf8Error(std::string::FromUtf8Error),
    #[error("Request error: {0}")]
    Reqwest(reqwest::Error),
    #[error("Storage `{0}` not found")]
    StorageNotFound(&'static str),
    #[error("Json print/parse error: {0}")]
    SerdeError(serde_json::Error),
    #[error("Time error: {0}")]
    TimeError(std::time::SystemTimeError),
    #[error("Sync conflict in storage `{0}`")]
    Conflict(String),
}

#[derive(Serialize, Deserialize, Debug)]
struct GeneralConfig {
    status_path: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct PairConfig {
    a: String,
    b: String,
    #[serde(default)]
    conflict_resolution: ConflictResolution,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
enum ConflictResolution {
    #[serde(rename = "error")]
    Error,
    #[serde(rename = "a wins")]
    AWins,
    #[serde(rename = "b wins")]
    BWins,
}

impl Default for ConflictResolution {
    fn default() -> ConflictResolution {
        ConflictResolution::Error
    }
}

#[derive(Serialize, Deserialize, Debug)]
struct StorageConfig {
    #[serde(rename = "type")]
    _type: StorageType,
    url: Option<String>,
    path: Option<String>,
    username: Option<Command>,
    password: Option<Command>,
}

#[derive(Serialize, Deserialize, Debug)]
enum StorageType {
    #[serde(rename = "nextcloud")]
    Nextcloud,
    #[serde(rename = "file")]
    File,
}

#[derive(Serialize, Deserialize, Debug)]
struct Command {
    fetch: Vec<String>,
}

impl Command {
    pub fn value(&self) -> Result<String, Error> {
        let sh = if cfg!(target_os = "windows") {
            std::process::Command::new(&self.fetch[1])
                .args(&self.fetch[2..])
                .output()
        } else {
            std::process::Command::new(&self.fetch[1])
                .args(&self.fetch[2..])
                .output()
        };

        let output = sh.map_err(Error::IOError)?;

        let output = String::from_utf8(output.stdout).map_err(Error::Utf8Error)?;

        Ok(output.trim_end().to_string())
    }
}

#[derive(Serialize, Deserialize, Debug)]
struct Config {
    general: GeneralConfig,
    pair: HashMap<String, PairConfig>,
    storage: HashMap<String, StorageConfig>,
}

// STATE MODEL
type Path = String;
type Url = String;

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Bookmark {
    id: usize,
    name: Path,
    url: Url,
    lastmodified: UnixTime,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct SBookmark {
    name: Path,
    url: Url,
    lastmodified: UnixTime,
}

#[derive(Debug)]
struct Snapshot {
    #[allow(unused_variables, dead_code)]
    at: UnixTime,
    bookmarks: Vec<SBookmark>,
}

#[derive(Debug, Default)]
struct Changes {
    new: HashMap<String, Bookmark>,
    changed: HashMap<String, Bookmark>,
    deleted: HashMap<String, usize>,
}

#[derive(Debug)]
struct Update {
    a: Changes,
    b: Changes,
    new_state: Vec<SBookmark>,
}

// PAIR
#[derive(Debug)]
struct Pair {
    state_file: String,
    a: Storage,
    b: Storage,
    conflict_resolution: ConflictResolution,
    previous_state: Option<Snapshot>,
}

impl Pair {
    /// The changes to be applied to a (first) and b (snd) to obtain the new snapshot
    fn changes(&self) -> Result<Update, Error> {
        let a = self.a.list()?;
        let b = self.b.list()?;

        match &self.previous_state {
            Some(snapshot) => self.changes_with_snapshot(a, b, &snapshot),
            None => self.changes_initial(a, b),
        }
    }

    fn changes_initial(&self, a: Vec<Bookmark>, b: Vec<Bookmark>) -> Result<Update, Error> {
        let a_is_master = ConflictResolution::AWins == self.conflict_resolution;
        let (mut master, mut slave) = if a_is_master { (a, b) } else { (b, a) };

        let mut new_state: HashMap<String, Bookmark> = master
            .drain(..)
            .map(|bookmark| return (bookmark.name.clone(), bookmark))
            .collect();

        let slave_keys: HashSet<String> =
            slave.iter().map(|bookmark| bookmark.name.clone()).collect();

        let mut changes_master = Changes::default();
        let mut changes_slave = Changes::default();

        for entry_slave in slave.drain(..) {
            let entry = new_state.entry(entry_slave.name.clone());

            match entry {
                Entry::Occupied(entry_master) => {
                    // If both urls are equal, there is nothing to do
                    if entry_master.get().url != entry_slave.url {
                        // Here we have a conflict
                        if let ConflictResolution::Error = self.conflict_resolution {
                            Err(Error::Conflict(entry_slave.name))
                        } else {
                            // Otherwise, master wins
                            changes_slave.changed.insert(
                                entry_slave.name,
                                Bookmark {
                                    id: entry_slave.id,
                                    ..(entry_master.get().clone())
                                },
                            );
                            Ok(())
                        }
                    } else {
                        Ok(())
                    }
                }
                Entry::Vacant(_) => {
                    // The entry was not in the master
                    changes_master.new.insert(
                        entry_slave.name.clone(),
                        Bookmark {
                            id: 0,
                            ..entry_slave.clone()
                        },
                    );
                    entry.or_insert(entry_slave);
                    Ok(())
                }
            }?;
        }

        // Finally we need to handle the bookmarks which are in master, but not in slave
        for (key, entry_master) in new_state.iter() {
            if !slave_keys.contains(&key.clone()) {
                changes_slave.new.insert(
                    key.clone(),
                    Bookmark {
                        id: 0,
                        ..entry_master.clone()
                    },
                );
            }
        }

        let (changes_a, changes_b) = if a_is_master {
            (changes_master, changes_slave)
        } else {
            (changes_slave, changes_master)
        };
        Ok(Update {
            a: changes_a,
            b: changes_b,
            new_state: new_state
                .drain()
                .map(|(_, v)| SBookmark {
                    url: v.url,
                    name: v.name,
                    lastmodified: v.lastmodified,
                })
                .collect(),
        })
    }

    fn changes_with_snapshot(
        &self,
        a: Vec<Bookmark>,
        b: Vec<Bookmark>,
        snapshot: &Snapshot,
    ) -> Result<Update, Error> {
        let a_ids: HashMap<String, usize> = a
            .iter()
            .map(|bookmark| (bookmark.name.clone(), bookmark.id))
            .collect();
        let b_ids: HashMap<String, usize> = b
            .iter()
            .map(|bookmark| (bookmark.name.clone(), bookmark.id))
            .collect();

        let mut new_state_hash: HashMap<String, Bookmark> = a
            .iter()
            .map(|bookmark| (bookmark.name.clone(), bookmark.clone()))
            .collect();

        let mut changes_a = Pair::compare_to_snapshot(a, snapshot);
        let mut changes_b = Pair::compare_to_snapshot(b, snapshot);

        // Remove conflicts
        self.handle_duplicates(&mut changes_a.new, &mut changes_b.new)?;
        self.handle_duplicates(&mut changes_a.changed, &mut changes_b.changed)?;
        self.handle_duplicates(&mut changes_a.deleted, &mut changes_b.deleted)?;

        // changes_a need to be applied on b and vice versa
        self.change_ids(&mut changes_a, &b_ids);
        self.change_ids(&mut changes_b, &a_ids);

        // Apply changes to our local state
        for (key, val) in changes_b.new.iter() {
            new_state_hash.insert(key.clone(), val.clone());
        }
        for (key, val) in changes_b.changed.iter() {
            new_state_hash.insert(key.clone(), val.clone());
        }
        for (key, _) in changes_b.deleted.iter() {
            new_state_hash.remove(key);
        }

        let new_state: Vec<SBookmark> = new_state_hash
            .drain()
            .map(|(_, bookmark)| SBookmark {
                name: bookmark.name,
                url: bookmark.url,
                lastmodified: bookmark.lastmodified,
            })
            .collect();

        // Then these changes can applied on the other pair
        Ok(Update {
            a: changes_b,
            b: changes_a,
            new_state,
        })
    }

    fn change_ids(&self, changes: &mut Changes, new_ids: &HashMap<String, usize>) {
        for (key, val) in changes.changed.iter_mut() {
            // The key must exist in new_ids, because otherwise it would not be in the updates
            let new_id = new_ids.get(key).unwrap();
            val.id = *new_id;
        }

        for (key, id) in changes.deleted.iter_mut() {
            let new_id = new_ids.get(key).unwrap();
            *id = *new_id;
        }
    }

    fn handle_duplicates<T>(
        &self,
        a: &mut HashMap<String, T>,
        b: &mut HashMap<String, T>,
    ) -> Result<(), Error> {
        match &self.conflict_resolution {
            ConflictResolution::AWins => {
                for (key, _) in a {
                    if b.contains_key(key) {
                        b.remove(key);
                    }
                }
                Ok(())
            }
            ConflictResolution::BWins => {
                for (key, _) in b {
                    if a.contains_key(key) {
                        a.remove(key);
                    }
                }
                Ok(())
            }
            ConflictResolution::Error => {
                for (key, _) in b {
                    if a.contains_key(key) {
                        Err(Error::Conflict(key.clone()))?;
                    };
                }
                Ok(())
            }
        }?;

        Ok(())
    }

    fn compare_to_snapshot(a: Vec<Bookmark>, snapshot: &Snapshot) -> Changes {
        let a_keys: HashSet<String> = a.iter().map(|bookmark| bookmark.name.clone()).collect();

        let mut snapshot_hash: HashMap<String, &SBookmark> = snapshot
            .bookmarks
            .iter()
            .map(|bookmark| return (bookmark.name.clone(), bookmark))
            .collect();

        let mut new: HashMap<String, Bookmark> = HashMap::new();
        let mut changed: HashMap<String, Bookmark> = HashMap::new();
        for bookmark in a {
            if let Some(old_bookmark) = snapshot_hash.get(&bookmark.name) {
                if old_bookmark.url != bookmark.url {
                    // Updated
                    changed.insert(bookmark.name.clone(), bookmark);
                }
            } else {
                // New
                new.insert(bookmark.name.clone(), bookmark);
            }
        }

        // Deleted
        let mut deleted: HashMap<String, usize> = HashMap::new();
        for (key, _) in snapshot_hash.drain() {
            if !a_keys.contains(&key) {
                deleted.insert(key, 0);
            }
        }

        Changes {
            deleted,
            new,
            changed,
        }
    }

    pub fn new(
        state_file: String,
        cfg: &PairConfig,
        cfg_a: &StorageConfig,
        cfg_b: &StorageConfig,
    ) -> Result<Pair, Error> {
        let a = Storage::from_config(cfg_a)?;
        let b = Storage::from_config(cfg_b)?;

        let previous_state = Pair::read_state(&state_file)?;

        Ok(Pair {
            state_file,
            a,
            b,
            conflict_resolution: cfg.conflict_resolution.clone(),
            previous_state,
        })
    }

    pub fn run(&mut self) -> Result<(), Error> {
        let update = self.changes()?;

        self.a.apply(update.a, &update.new_state)?;
        self.b.apply(update.b, &update.new_state)?;

        self.write_state(update.new_state)
    }

    fn read_state(state_file: &str) -> Result<Option<Snapshot>, Error> {
        let result = fs::read_to_string(state_file);

        match result {
            Ok(cnt) => {
                let bookmarks: Vec<SBookmark> =
                    serde_json::from_str(&cnt).map_err(Error::SerdeError)?;

                let at = FileStorage::file_modified(state_file)?;

                Ok(Some(Snapshot { bookmarks, at }))
            }
            Err(error) => match error.kind() {
                std::io::ErrorKind::NotFound => Ok(None),
                _ => Err(Error::IOError(error)),
            },
        }
    }

    fn write_state(&self, bookmarks: Vec<SBookmark>) -> Result<(), Error> {
        let path = std::path::Path::new(&self.state_file);
        let parent = path.parent().unwrap();

        if !parent.exists() {
            let _ = fs::create_dir_all(parent).map_err(Error::IOError)?;
        }

        let bytes = serde_json::to_string(&bookmarks)
            .map_err(Error::SerdeError)?
            .into_bytes();

        let mut f = fs::File::create(path).map_err(Error::IOError)?;
        f.write_all(&bytes).map_err(Error::IOError)?;

        Ok(())
    }
}

// STORAGE
#[derive(Debug)]
enum Storage {
    File(FileStorage),
    Nextcloud(NextcloudStorage),
}

impl Storage {
    pub fn apply(&mut self, changes: Changes, new_state: &Vec<SBookmark>) -> Result<(), Error> {
        match self {
            Storage::File(fs_storage) => fs_storage.apply(changes, new_state),
            Storage::Nextcloud(nc_storage) => nc_storage.apply(changes, new_state),
        }
    }

    pub fn list(&self) -> Result<Vec<Bookmark>, Error> {
        match self {
            Storage::File(fs_storage) => fs_storage.list(),
            Storage::Nextcloud(nc_storage) => nc_storage.list(),
        }
    }

    pub fn from_config(cfg: &StorageConfig) -> Result<Storage, Error> {
        match cfg._type {
            StorageType::Nextcloud => Storage::from_config_nc(cfg),
            StorageType::File => {
                if let Some(path) = &cfg.path {
                    Ok(Storage::File(FileStorage {
                        path: path.to_owned(),
                    }))
                } else {
                    Err(Error::MissingConfig("path"))
                }
            }
        }
    }

    fn from_config_nc(cfg: &StorageConfig) -> Result<Storage, Error> {
        let url = cfg.url.as_ref().ok_or(Error::MissingConfig("url"))?;
        let username_cmd = cfg
            .username
            .as_ref()
            .ok_or(Error::MissingConfig("username"))?;
        let passwd_cmd = cfg
            .password
            .as_ref()
            .ok_or(Error::MissingConfig("password"))?;

        let username = username_cmd.value()?;
        let password = passwd_cmd.value()?;

        NextcloudStorage::new(url.to_owned(), username.to_owned(), password.to_owned())
            .map(Storage::Nextcloud)
    }
}

// FILESYSTEM
#[derive(Debug)]
struct FileStorage {
    path: String,
}

impl FileStorage {
    pub fn apply(&self, _changes: Changes, new_state: &Vec<SBookmark>) -> Result<(), Error> {
        let path = std::path::Path::new(&self.path);
        let parent = path.parent().unwrap();

        if !parent.exists() {
            let _ = fs::create_dir_all(parent).map_err(Error::IOError)?;
        }

        let lines: String = new_state
            .iter()
            .map(|bookmark| {
                let line: String = bookmark.name.clone() + " " + &bookmark.url + "\n";
                line
            })
            .collect();

        let bytes = lines.into_bytes();

        let mut f = fs::File::create(path).map_err(Error::IOError)?;
        f.write_all(&bytes).map_err(Error::IOError)?;

        Ok(())
    }

    pub fn list(&self) -> Result<Vec<Bookmark>, Error> {
        let result = fs::read_to_string(&self.path);
        let lastmodified = FileStorage::file_modified(&self.path)?;

        match result {
            Ok(cnt) => Ok(FileStorage::read_file_content(lastmodified, cnt)),
            Err(error) => match error.kind() {
                std::io::ErrorKind::NotFound => Ok(Vec::new()),
                _ => Err(Error::IOError(error)),
            },
        }
    }

    fn read_file_content(lastmodified: UnixTime, cnt: String) -> Vec<Bookmark> {
        cnt.lines()
            .enumerate()
            .map(|(i, ln)| {
                let lastspace = ln.rfind(char::is_whitespace).unwrap_or(0);

                let (name, url) = ln.split_at(lastspace);

                Bookmark {
                    id: i,
                    name: name.to_owned(),
                    url: url.trim().to_owned(),
                    lastmodified,
                }
            })
            .collect()
    }

    pub fn file_modified(state_file: &str) -> Result<UnixTime, Error> {
        let at = fs::metadata(state_file)
            .map_err(Error::IOError)?
            .modified()
            .map_err(Error::IOError)?
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .map_err(Error::TimeError)?
            .as_secs();
        Ok(at)
    }
}

// NEXTCLOUD
#[derive(Serialize, Deserialize, Debug)]
struct List<T> {
    data: Vec<T>,
}

#[derive(Serialize, Deserialize, Debug)]
struct NcBookmark {
    id: usize,
    title: String,
    url: String,
    lastmodified: UnixTime,
    folders: Vec<i32>,
}

#[derive(Serialize, Debug)]
struct NewNcBookmark {
    title: String,
    url: String,
    folders: Vec<i32>,
}

#[derive(Serialize, Debug)]
struct ChangedNcBookmark {
    url: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct NcFolder {
    id: i32,
    title: String,
    parent_folder: i32,
    children: Vec<NcFolder>,
}

#[derive(Serialize, Debug)]
struct NewNcFolder {
    title: String,
    parent_folder: i32,
}

#[derive(Deserialize, Debug)]
struct Item<T> {
    item: T,
}

#[derive(Deserialize, Debug)]
struct Id<T> {
    id: T,
}

#[derive(Debug)]
struct NextcloudStorage {
    url: String,
    username: String,
    password: String,
    folders: Vec<NcFolder>,
}

impl NextcloudStorage {
    pub fn apply(
        &mut self,
        mut changes: Changes,
        _new_state: &Vec<SBookmark>,
    ) -> Result<(), Error> {
        let mut parent = NcFolder {
            title: String::new(),
            id: -1,
            children: self.folders.clone(),
            parent_folder: -2,
        };

        for (_, bookmark) in changes.new.drain() {
            let exploded: Vec<&str> = bookmark.name.split('/').collect();
            let len = exploded.len();
            let folder_id = self.ensure_folder(&mut parent, &exploded[..len - 1])?;

            self.add_bookmark(folder_id, bookmark)?;
        }

        for (_, bookmark) in changes.changed.drain() {
            self.edit_bookmark(bookmark)?;
        }

        for bookmark in changes.deleted.values() {
            self.delete_bookmark(*bookmark)?;
        }

        Ok(())
    }

    fn add_bookmark(&self, folder_id: i32, bookmark: Bookmark) -> Result<(), Error> {
        let lastslash = bookmark
            .name
            .rfind(|c| c == '/')
            .map(|x| x + 1)
            .unwrap_or(0);
        let (_, title) = bookmark.name.split_at(lastslash);

        let new_bookmark = NewNcBookmark {
            url: bookmark.url,
            title: title.to_string(),
            folders: vec![folder_id],
        };

        let client = reqwest::blocking::Client::new();

        let bookmark_url = self.url.clone() + "/bookmark";
        let _ = client
            .post(&bookmark_url)
            .basic_auth(&self.username, Some(&self.password))
            .json(&new_bookmark)
            .send()
            .map_err(Error::Reqwest)?;

        Ok(())
    }

    fn edit_bookmark(&self, bookmark: Bookmark) -> Result<(), Error> {
        let updated_bookmark = ChangedNcBookmark { url: bookmark.url };

        let client = reqwest::blocking::Client::new();

        let bookmark_url = self.url.clone() + "/bookmark/" + &bookmark.id.to_string();
        let _ = client
            .put(&bookmark_url)
            .basic_auth(&self.username, Some(&self.password))
            .json(&updated_bookmark)
            .send()
            .map_err(Error::Reqwest)?;

        Ok(())
    }

    fn delete_bookmark(&self, bookmark_id: usize) -> Result<(), Error> {
        let client = reqwest::blocking::Client::new();

        let bookmark_url = self.url.clone() + "/bookmark/" + &bookmark_id.to_string();
        let _ = client
            .delete(&bookmark_url)
            .basic_auth(&self.username, Some(&self.password))
            .send()
            .map_err(Error::Reqwest)?;

        Ok(())
    }

    fn ensure_folder(&self, folder: &mut NcFolder, parts: &[&str]) -> Result<i32, Error> {
        if let Some(head) = parts.first() {
            for child in folder.children.iter_mut() {
                if &child.title == head {
                    return self.ensure_folder(child, &parts[1..]);
                }
            }
            return self.add_subfolders(folder, &parts);
        } else {
            Ok(folder.id)
        }
    }

    fn add_subfolders(&self, parent: &mut NcFolder, paths: &[&str]) -> Result<i32, Error> {
        if paths.is_empty() {
            return Ok(parent.id);
        }

        let mut parent_folder = parent.id;

        let mut folders = Vec::new();
        for title in paths {
            let id = self.add_subfolder(NewNcFolder {
                parent_folder,
                title: title.to_string(),
            })?;
            folders.push(NcFolder {
                id,
                parent_folder,
                title: title.to_string(),
                children: Vec::new(),
            });
            parent_folder = id;
        }

        let mut top_folder = folders.pop().unwrap();
        while let Some(mut new_top_folder) = folders.pop() {
            new_top_folder.children.push(top_folder);
            top_folder = new_top_folder;
        }

        parent.children.push(top_folder);

        Ok(parent_folder)
    }

    fn add_subfolder(&self, folder: NewNcFolder) -> Result<i32, Error> {
        let client = reqwest::blocking::Client::new();

        let folder_url = self.url.clone() + "/folder";
        let result: Item<Id<i32>> = client
            .post(&folder_url)
            .basic_auth(&self.username, Some(&self.password))
            .json(&folder)
            .send()
            .map_err(Error::Reqwest)?
            .json()
            .map_err(Error::Reqwest)?;

        Ok(result.item.id)
    }

    pub fn new(url: String, username: String, password: String) -> Result<NextcloudStorage, Error> {
        let client = reqwest::blocking::Client::new();

        let folder_url = url.clone() + "/folder";
        let folders: List<NcFolder> = client
            .get(&folder_url)
            .basic_auth(&username, Some(&password))
            .send()
            .map_err(Error::Reqwest)?
            .json()
            .map_err(Error::Reqwest)?;

        Ok(NextcloudStorage {
            url,
            username,
            password,
            folders: folders.data,
        })
    }

    pub fn list(&self) -> Result<Vec<Bookmark>, Error> {
        let client = reqwest::blocking::Client::new();

        let bookmark_url = self.url.clone() + "/bookmark";
        let mut bookmarks: List<NcBookmark> = client
            .get(&bookmark_url)
            .query(&[("limit", 10000)])
            .basic_auth(&self.username, Some(&self.password))
            .send()
            .map_err(Error::Reqwest)?
            .json()
            .map_err(Error::Reqwest)?;

        Ok(bookmarks
            .data
            .drain(..)
            .map(|bookmark| {
                let mut name = bookmark.title;

                if let Some(folder_id) = bookmark.folders.first() {
                    if let Some(path) = NextcloudStorage::folder_path(&self.folders, *folder_id) {
                        name = path + "/" + &name;
                    }
                }
                Bookmark {
                    id: bookmark.id,
                    name: name,
                    url: bookmark.url,
                    lastmodified: bookmark.lastmodified,
                }
            })
            .collect())
    }

    fn folder_path(folders: &Vec<NcFolder>, id: i32) -> Option<String> {
        for folder in folders {
            if folder.id == id {
                return Some(folder.title.clone());
            } else if let Some(end) = NextcloudStorage::folder_path(&folder.children, id) {
                let path = folder.title.clone() + "/" + &end;
                return Some(path);
            }
        }
        None
    }
}
