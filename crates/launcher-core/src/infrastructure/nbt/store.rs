use std::io;
use std::path::PathBuf;

use crate::domain::instance::InstanceId;
use crate::domain::launch::LauncherPaths;
use crate::domain::server_list::{ServerList, ServerListStore, ServerListStoreError};
use crate::infrastructure::fs::write_atomic;

use super::codec;

const FILE_NAME: &str = "servers.dat";

/// `servers.dat` in the game directory of each instance, as uncompressed NBT.
#[derive(Debug, Clone)]
pub struct NbtServerListStore {
    paths: LauncherPaths,
}

impl NbtServerListStore {
    pub fn new(paths: LauncherPaths) -> Self {
        Self { paths }
    }

    fn file(&self, instance: &InstanceId) -> PathBuf {
        self.paths.game_dir(instance).join(FILE_NAME)
    }
}

impl ServerListStore for NbtServerListStore {
    async fn load(&self, instance: &InstanceId) -> Result<ServerList, ServerListStoreError> {
        let path = self.file(instance);
        let bytes = match tokio::fs::read(&path).await {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(ServerList::default());
            }
            Err(error) => return Err(io_error(&path, &error)),
        };
        codec::decode(&bytes)
    }

    async fn save(
        &self,
        instance: &InstanceId,
        list: &ServerList,
    ) -> Result<(), ServerListStoreError> {
        let bytes = codec::encode(list)?;
        let path = self.file(instance);
        write_atomic(&path, &bytes)
            .await
            .map_err(|error| io_error(&path, &error))
    }
}

fn io_error(path: &std::path::Path, error: &io::Error) -> ServerListStoreError {
    ServerListStoreError::Io(format!("{}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use fastnbt::Value;

    use super::*;
    use crate::domain::server_list::{ServerAddress, ServerEntry};

    const FIXTURE: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/nbt/servers-with-unknown-tags.dat"
    ));

    struct Setup {
        _dir: tempfile::TempDir,
        store: NbtServerListStore,
        instance: InstanceId,
        file: PathBuf,
    }

    fn setup(contents: Option<&[u8]>) -> Setup {
        let dir = tempfile::tempdir().unwrap();
        let paths = LauncherPaths::new(dir.path());
        let instance = InstanceId::parse("survival").unwrap();
        let file = paths.game_dir(&instance).join("servers.dat");
        if let Some(bytes) = contents {
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(&file, bytes).unwrap();
        }
        Setup {
            _dir: dir,
            store: NbtServerListStore::new(paths),
            instance,
            file,
        }
    }

    fn decoded(bytes: &[u8]) -> Value {
        fastnbt::from_bytes(bytes).unwrap()
    }

    fn compound(value: &Value) -> &HashMap<String, Value> {
        match value {
            Value::Compound(map) => map,
            other => panic!("expected a compound, got {other:?}"),
        }
    }

    fn servers(value: &Value) -> Vec<&HashMap<String, Value>> {
        match &compound(value)["servers"] {
            Value::List(items) => items.iter().map(compound).collect(),
            other => panic!("expected a list, got {other:?}"),
        }
    }

    fn names(list: &ServerList) -> Vec<&str> {
        list.entries().iter().map(|e| e.name.as_str()).collect()
    }

    fn preserved_names(fields: &crate::domain::server_list::Preserved) -> Vec<&str> {
        fields.0.iter().map(|f| f.name.as_str()).collect()
    }

    #[tokio::test]
    async fn loads_the_fixture_with_known_fields_and_preserved_tags() {
        let s = setup(Some(FIXTURE));
        let list = s.store.load(&s.instance).await.unwrap();

        let entries = list.entries();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].accept_textures, Some(true));
        assert!(!entries[0].hidden);
        assert_eq!(entries[0].icon, None);
        assert_eq!(preserved_names(&entries[0].preserved), ["forgeData"]);
        assert_eq!(entries[0].preserved.0[0].kind, 10);
        assert_eq!(entries[1].address.to_string(), "play.example.net:25570");
        assert_eq!(entries[1].accept_textures, None);
        assert!(!entries[1].hidden);
        assert!(entries[1].icon.is_some());
        // The raw `hidden` tag is kept so its presence survives a save.
        assert_eq!(preserved_names(&entries[1].preserved), ["hidden"]);
        assert_eq!(preserved_names(&list.preserved), ["launcherNote"]);
        assert_eq!(list.preserved.0[0].kind, 3);

        // Names and the first address come straight from the fixture's tree.
        let tree = decoded(FIXTURE);
        let raw = servers(&tree);
        for (entry, raw) in entries.iter().zip(&raw) {
            assert_eq!(raw["name"], Value::String(entry.name.clone()));
        }
        assert_eq!(raw[0]["ip"], Value::String(entries[0].address.to_string()));
    }

    #[tokio::test]
    async fn a_round_trip_keeps_every_tag_and_value() {
        let s = setup(Some(FIXTURE));
        let list = s.store.load(&s.instance).await.unwrap();
        std::fs::remove_file(&s.file).unwrap();
        s.store.save(&s.instance, &list).await.unwrap();

        let saved = std::fs::read(&s.file).unwrap();
        assert_eq!(decoded(&saved), decoded(FIXTURE));
    }

    #[tokio::test]
    async fn a_missing_file_is_an_empty_list() {
        let s = setup(None);
        assert_eq!(
            s.store.load(&s.instance).await.unwrap(),
            ServerList::default()
        );
    }

    #[tokio::test]
    async fn truncated_or_garbage_bytes_are_corrupt() {
        for bytes in [&FIXTURE[..FIXTURE.len() / 2], b"not nbt at all".as_slice()] {
            let s = setup(Some(bytes));
            assert!(matches!(
                s.store.load(&s.instance).await,
                Err(ServerListStoreError::Corrupt(_))
            ));
        }
    }

    #[tokio::test]
    async fn a_root_that_is_not_a_compound_is_corrupt() {
        // A named Int root ("", 42) instead of a compound.
        let s = setup(Some(&[3, 0, 0, 0, 0, 0, 42]));
        assert!(matches!(
            s.store.load(&s.instance).await,
            Err(ServerListStoreError::Corrupt(_))
        ));
    }

    #[tokio::test]
    async fn servers_that_is_not_a_list_of_compounds_is_corrupt() {
        let mut root = HashMap::new();
        root.insert("servers".to_owned(), Value::Int(1));
        let s = setup(Some(&fastnbt::to_bytes(&Value::Compound(root)).unwrap()));
        assert!(matches!(
            s.store.load(&s.instance).await,
            Err(ServerListStoreError::Corrupt(_))
        ));
    }

    #[tokio::test]
    async fn an_unparseable_ip_keeps_the_raw_entry_in_place() {
        let entry = |name: &str, ip: &str| {
            let mut map = HashMap::new();
            map.insert("name".to_owned(), Value::String(name.to_owned()));
            map.insert("ip".to_owned(), Value::String(ip.to_owned()));
            Value::Compound(map)
        };
        let mut root = HashMap::new();
        root.insert(
            "servers".to_owned(),
            Value::List(vec![
                entry("Good", "a.example"),
                entry("Broken", "bad host!"),
                entry("Also good", "b.example"),
            ]),
        );
        let original = fastnbt::to_bytes(&Value::Compound(root)).unwrap();
        let s = setup(Some(&original));

        let list = s.store.load(&s.instance).await.unwrap();
        assert_eq!(names(&list), ["Good", "Also good"]);
        s.store.save(&s.instance, &list).await.unwrap();
        assert_eq!(
            decoded(&std::fs::read(&s.file).unwrap()),
            decoded(&original)
        );
    }

    #[tokio::test]
    async fn domain_edits_persist_and_keep_unknown_tags() {
        let s = setup(Some(FIXTURE));
        let mut list = s.store.load(&s.instance).await.unwrap();
        let first_name = list.entries()[0].name.clone();
        let second_name = list.entries()[1].name.clone();

        let cinepol = ServerEntry::new(
            "CINEPOL",
            ServerAddress::parse("mc.policarpiers.com").unwrap(),
        );
        assert!(list.seed_first(cinepol));
        s.store.save(&s.instance, &list).await.unwrap();

        let list = s.store.load(&s.instance).await.unwrap();
        assert_eq!(names(&list), ["CINEPOL", &first_name, &second_name]);
        assert_eq!(list.entries()[0].address.to_string(), "mc.policarpiers.com");
        assert_eq!(preserved_names(&list.entries()[1].preserved), ["forgeData"]);
        assert_eq!(preserved_names(&list.preserved), ["launcherNote"]);
        let tree = decoded(&std::fs::read(&s.file).unwrap());
        let raw = servers(&tree);
        assert!(!raw[0].contains_key("acceptTextures"));
        assert!(!raw[0].contains_key("icon"));
        assert!(!raw[0].contains_key("hidden"));
        assert_eq!(raw[2]["hidden"], Value::Byte(0));
        assert_eq!(
            compound(&raw[1]["forgeData"])["fmlNetworkVersion"],
            Value::Int(3)
        );

        let mut list = list;
        list.move_entry(2, 0).unwrap();
        let mut edited = list.entries()[1].clone();
        edited.name = "Renamed".into();
        edited.hidden = true;
        edited.accept_textures = Some(false);
        list.replace(1, edited).unwrap();
        list.remove(2).unwrap();
        s.store.save(&s.instance, &list).await.unwrap();

        let list = s.store.load(&s.instance).await.unwrap();
        assert_eq!(names(&list), [second_name.as_str(), "Renamed"]);
        assert!(list.entries()[1].hidden);
        assert_eq!(list.entries()[1].accept_textures, Some(false));
        assert_eq!(list.entries()[1].address.to_string(), "mc.policarpiers.com");
        assert!(list.entries()[0].icon.is_some());
        assert_eq!(preserved_names(&list.preserved), ["launcherNote"]);
    }

    #[tokio::test]
    async fn saving_creates_a_missing_game_directory() {
        let s = setup(None);
        assert!(!s.file.parent().unwrap().exists());
        let mut list = ServerList::default();
        list.add(ServerEntry::new(
            "A",
            ServerAddress::parse("a.example").unwrap(),
        ));
        s.store.save(&s.instance, &list).await.unwrap();
        assert_eq!(names(&s.store.load(&s.instance).await.unwrap()), ["A"]);

        s.store
            .save(&s.instance, &ServerList::default())
            .await
            .unwrap();
        assert_eq!(
            s.store.load(&s.instance).await.unwrap(),
            ServerList::default()
        );
    }

    #[tokio::test]
    async fn an_unreadable_path_is_an_io_error() {
        let s = setup(None);
        // A directory where the file should be cannot be read as a file.
        std::fs::create_dir_all(&s.file).unwrap();
        assert!(matches!(
            s.store.load(&s.instance).await,
            Err(ServerListStoreError::Io(_))
        ));
        assert!(matches!(
            s.store.save(&s.instance, &ServerList::default()).await,
            Err(ServerListStoreError::Io(_))
        ));
    }
}
