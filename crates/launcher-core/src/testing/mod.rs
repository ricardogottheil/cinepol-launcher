//! In-memory fakes for every port, for unit tests here and in dependent crates
//! (enable the `test-support` feature). No I/O, no network, no real time.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

use crate::domain::account::{Account, AccountId, AccountRepository, AccountRepositoryError};
use crate::domain::instance::{
    GameVersion, Instance, InstanceId, InstanceRepository, InstanceRepositoryError,
};
use crate::domain::java::{JavaProbe, JavaProbeError, JavaVersion};
use crate::domain::loader::{LoaderKind, LoaderMetadata, LoaderMetadataError, LoaderRelease};
use crate::domain::modpack::{
    ModpackPage, ModpackQuery, ModpackSource, ModpackSourceError, ModpackSourceKind, ModpackVersion,
};
use crate::domain::ports::{
    Clock, Delay, DownloadError, DownloadRequest, DownloadSummary, Downloader, HttpClient,
    HttpError, ProgressReporter, ProgressUpdate, Secret, SecretKey, SecretStore, SecretStoreError,
};
use crate::domain::server_list::{ServerList, ServerListStore, ServerListStoreError};
use tokio_util::sync::CancellationToken;

mod launch;
pub use launch::{
    FakeFileSystem, FakeGameProcess, FakeJavaRuntimes, FakeVersionSource, RecordingLogSink,
    RecordingNativesExtractor, sample_asset_index, sample_version_spec,
};

/// Records every progress update.
#[derive(Debug, Default)]
pub struct RecordingReporter {
    updates: Mutex<Vec<ProgressUpdate>>,
}

impl RecordingReporter {
    pub fn updates(&self) -> Vec<ProgressUpdate> {
        self.updates.lock().unwrap().clone()
    }
}

impl ProgressReporter for RecordingReporter {
    fn report(&self, update: ProgressUpdate) {
        self.updates.lock().unwrap().push(update);
    }
}

/// Returns immediately and counts how often it was asked to wait.
#[derive(Debug, Default)]
pub struct InstantDelay {
    waits: Mutex<Vec<Duration>>,
}

impl InstantDelay {
    pub fn waits(&self) -> Vec<Duration> {
        self.waits.lock().unwrap().clone()
    }
}

impl Delay for InstantDelay {
    async fn wait(&self, duration: Duration) {
        self.waits.lock().unwrap().push(duration);
    }
}

/// A clock that only moves when told to.
#[derive(Debug)]
pub struct FixedClock {
    now: Mutex<SystemTime>,
}

impl FixedClock {
    pub fn at_unix(secs: u64) -> Self {
        Self {
            now: Mutex::new(SystemTime::UNIX_EPOCH + Duration::from_secs(secs)),
        }
    }

    pub fn advance(&self, by: Duration) {
        *self.now.lock().unwrap() += by;
    }
}

impl Clock for FixedClock {
    fn now(&self) -> SystemTime {
        *self.now.lock().unwrap()
    }
}

/// Keychain stand-in.
#[derive(Debug, Default)]
pub struct InMemorySecretStore {
    secrets: Mutex<HashMap<SecretKey, Secret>>,
}

impl SecretStore for InMemorySecretStore {
    async fn get(&self, key: &SecretKey) -> Result<Option<Secret>, SecretStoreError> {
        Ok(self.secrets.lock().unwrap().get(key).cloned())
    }

    async fn set(&self, key: &SecretKey, secret: &Secret) -> Result<(), SecretStoreError> {
        self.secrets
            .lock()
            .unwrap()
            .insert(key.clone(), secret.clone());
        Ok(())
    }

    async fn delete(&self, key: &SecretKey) -> Result<(), SecretStoreError> {
        self.secrets.lock().unwrap().remove(key);
        Ok(())
    }
}

/// Account metadata store; `fail_writes` makes every write fail.
#[derive(Debug, Default)]
pub struct InMemoryAccountRepository {
    accounts: Mutex<Vec<Account>>,
    active: Mutex<Option<AccountId>>,
    fail_writes: AtomicBool,
    fail_activation: AtomicBool,
}

impl InMemoryAccountRepository {
    pub fn with(accounts: Vec<Account>) -> Self {
        Self {
            accounts: Mutex::new(accounts),
            ..Self::default()
        }
    }

    pub fn fail_writes(&self) {
        self.fail_writes.store(true, Ordering::SeqCst);
    }

    /// Only `set_active` fails; saves and removals still work.
    pub fn fail_activation(&self) {
        self.fail_activation.store(true, Ordering::SeqCst);
    }

    fn check_write(&self) -> Result<(), AccountRepositoryError> {
        if self.fail_writes.load(Ordering::SeqCst) {
            return Err(AccountRepositoryError::Storage("disk full".into()));
        }
        Ok(())
    }
}

impl AccountRepository for InMemoryAccountRepository {
    async fn list(&self) -> Result<Vec<Account>, AccountRepositoryError> {
        Ok(self.accounts.lock().unwrap().clone())
    }

    async fn save(&self, account: &Account) -> Result<(), AccountRepositoryError> {
        self.check_write()?;
        let mut accounts = self.accounts.lock().unwrap();
        match accounts
            .iter_mut()
            .find(|existing| existing.id == account.id)
        {
            Some(existing) => *existing = account.clone(),
            None => accounts.push(account.clone()),
        }
        Ok(())
    }

    async fn remove(&self, id: AccountId) -> Result<(), AccountRepositoryError> {
        self.check_write()?;
        self.accounts
            .lock()
            .unwrap()
            .retain(|account| account.id != id);
        Ok(())
    }

    async fn active(&self) -> Result<Option<AccountId>, AccountRepositoryError> {
        Ok(*self.active.lock().unwrap())
    }

    async fn set_active(&self, id: Option<AccountId>) -> Result<(), AccountRepositoryError> {
        self.check_write()?;
        if self.fail_activation.load(Ordering::SeqCst) {
            return Err(AccountRepositoryError::Storage("activation failed".into()));
        }
        *self.active.lock().unwrap() = id;
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct InMemoryInstanceRepository {
    instances: Mutex<Vec<Instance>>,
}

impl InstanceRepository for InMemoryInstanceRepository {
    async fn list(&self) -> Result<Vec<Instance>, InstanceRepositoryError> {
        Ok(self.instances.lock().unwrap().clone())
    }

    async fn get(&self, id: &InstanceId) -> Result<Instance, InstanceRepositoryError> {
        self.instances
            .lock()
            .unwrap()
            .iter()
            .find(|instance| &instance.id == id)
            .cloned()
            .ok_or_else(|| InstanceRepositoryError::NotFound(id.clone()))
    }

    async fn save(&self, instance: &Instance) -> Result<(), InstanceRepositoryError> {
        let mut instances = self.instances.lock().unwrap();
        match instances
            .iter_mut()
            .find(|existing| existing.id == instance.id)
        {
            Some(existing) => *existing = instance.clone(),
            None => instances.push(instance.clone()),
        }
        Ok(())
    }

    async fn delete(&self, id: &InstanceId) -> Result<(), InstanceRepositoryError> {
        let mut instances = self.instances.lock().unwrap();
        let before = instances.len();
        instances.retain(|instance| &instance.id != id);
        if instances.len() == before {
            return Err(InstanceRepositoryError::NotFound(id.clone()));
        }
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct InMemoryServerListStore {
    lists: Mutex<HashMap<InstanceId, ServerList>>,
}

impl ServerListStore for InMemoryServerListStore {
    async fn load(&self, instance: &InstanceId) -> Result<ServerList, ServerListStoreError> {
        Ok(self
            .lists
            .lock()
            .unwrap()
            .get(instance)
            .cloned()
            .unwrap_or_default())
    }

    async fn save(
        &self,
        instance: &InstanceId,
        list: &ServerList,
    ) -> Result<(), ServerListStoreError> {
        self.lists
            .lock()
            .unwrap()
            .insert(instance.clone(), list.clone());
        Ok(())
    }
}

/// Answers `java -version` from a table of executables.
#[derive(Debug, Default)]
pub struct FakeJavaProbe {
    runtimes: HashMap<PathBuf, JavaVersion>,
}

impl FakeJavaProbe {
    pub fn with(runtimes: impl IntoIterator<Item = (PathBuf, JavaVersion)>) -> Self {
        Self {
            runtimes: runtimes.into_iter().collect(),
        }
    }
}

impl JavaProbe for FakeJavaProbe {
    async fn probe(&self, executable: &Path) -> Result<JavaVersion, JavaProbeError> {
        self.runtimes
            .get(executable)
            .cloned()
            .ok_or_else(|| JavaProbeError::NotExecutable(executable.display().to_string()))
    }
}

/// A loader meta API with a fixed release table.
#[derive(Debug)]
pub struct FakeLoaderMetadata {
    pub kind: LoaderKind,
    pub releases: Vec<LoaderRelease>,
}

impl LoaderMetadata for FakeLoaderMetadata {
    fn kind(&self) -> LoaderKind {
        self.kind
    }

    async fn releases(
        &self,
        game_version: &GameVersion,
    ) -> Result<Vec<LoaderRelease>, LoaderMetadataError> {
        let matching: Vec<_> = self
            .releases
            .iter()
            .filter(|release| &release.game_version == game_version)
            .cloned()
            .collect();
        if matching.is_empty() {
            return Err(LoaderMetadataError::UnsupportedGameVersion(
                self.kind,
                game_version.as_str().to_owned(),
            ));
        }
        Ok(matching)
    }
}

/// A modpack catalog with one fixed page and per-project versions.
#[derive(Debug)]
pub struct FakeModpackSource {
    pub kind: ModpackSourceKind,
    pub page: ModpackPage,
    pub versions: HashMap<String, Vec<ModpackVersion>>,
}

impl ModpackSource for FakeModpackSource {
    fn kind(&self) -> ModpackSourceKind {
        self.kind
    }

    async fn search(&self, query: &ModpackQuery) -> Result<ModpackPage, ModpackSourceError> {
        let text = query.text.to_lowercase();
        let items = self
            .page
            .items
            .iter()
            .filter(|item| item.title.to_lowercase().contains(&text))
            .cloned()
            .collect();
        Ok(ModpackPage {
            items,
            total: self.page.total,
        })
    }

    async fn versions(&self, project: &str) -> Result<Vec<ModpackVersion>, ModpackSourceError> {
        self.versions
            .get(project)
            .cloned()
            .ok_or_else(|| ModpackSourceError::NotFound(project.to_owned()))
    }
}

/// Serves fixed bodies by URL; unknown URLs are 404. Records every request.
#[derive(Debug, Default)]
pub struct FakeHttpClient {
    bodies: HashMap<String, Vec<u8>>,
    requests: Mutex<Vec<String>>,
}

impl FakeHttpClient {
    pub fn with(bodies: impl IntoIterator<Item = (String, Vec<u8>)>) -> Self {
        Self {
            bodies: bodies.into_iter().collect(),
            requests: Mutex::default(),
        }
    }

    pub fn requests(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }
}

impl HttpClient for FakeHttpClient {
    async fn get_bytes(&self, url: &str) -> Result<Vec<u8>, HttpError> {
        self.requests.lock().unwrap().push(url.to_owned());
        self.bodies
            .get(url)
            .cloned()
            .ok_or_else(|| HttpError::NotFound(url.to_owned()))
    }
}

/// Writes fixed bodies by URL to each destination (creating parent folders)
/// without verifying checksums. Records every request it was given.
#[derive(Debug, Default)]
pub struct FakeDownloader {
    bodies: HashMap<String, Vec<u8>>,
    requests: Mutex<Vec<DownloadRequest>>,
}

impl FakeDownloader {
    pub fn with(bodies: impl IntoIterator<Item = (String, Vec<u8>)>) -> Self {
        Self {
            bodies: bodies.into_iter().collect(),
            requests: Mutex::default(),
        }
    }

    pub fn requests(&self) -> Vec<DownloadRequest> {
        self.requests.lock().unwrap().clone()
    }
}

impl Downloader for FakeDownloader {
    async fn download_all<R: ProgressReporter>(
        &self,
        requests: Vec<DownloadRequest>,
        reporter: &R,
        cancel: &CancellationToken,
    ) -> Result<DownloadSummary, DownloadError> {
        let mut summary = DownloadSummary::default();
        let total = requests.len() as u64;
        for (index, request) in requests.into_iter().enumerate() {
            if cancel.is_cancelled() {
                return Err(DownloadError::Cancelled);
            }
            let body = self.bodies.get(&request.url).cloned().unwrap_or_default();
            if let Some(parent) = request.destination.parent() {
                std::fs::create_dir_all(parent).map_err(|e| DownloadError::Io {
                    path: parent.display().to_string(),
                    reason: e.to_string(),
                })?;
            }
            std::fs::write(&request.destination, &body).map_err(|e| DownloadError::Io {
                path: request.destination.display().to_string(),
                reason: e.to_string(),
            })?;
            summary.downloaded += 1;
            summary.bytes += body.len() as u64;
            self.requests.lock().unwrap().push(request);
            reporter.report(ProgressUpdate::new(index as u64 + 1, Some(total)));
        }
        Ok(summary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::instance::{InstanceName, InstanceOrigin};
    use crate::domain::loader::{LoaderSpec, LoaderVersion};
    use crate::domain::modpack::ModpackSummary;
    use crate::domain::server_list::{ServerAddress, ServerEntry};

    fn game(version: &str) -> GameVersion {
        GameVersion::parse(version).unwrap()
    }

    #[tokio::test]
    async fn secret_store_round_trips_and_deletes() {
        let store = InMemorySecretStore::default();
        let key = SecretKey::new("msa-refresh:1");
        store.set(&key, &Secret::new("token")).await.unwrap();
        assert_eq!(store.get(&key).await.unwrap(), Some(Secret::new("token")));
        store.delete(&key).await.unwrap();
        assert_eq!(store.get(&key).await.unwrap(), None);
    }

    #[tokio::test]
    async fn instance_repository_saves_updates_and_reports_missing_ids() {
        let repo = InMemoryInstanceRepository::default();
        let id = InstanceId::parse("cinepol").unwrap();
        let mut instance = Instance::new(
            id.clone(),
            InstanceName::parse("CINEPOL").unwrap(),
            game("26.2"),
            LoaderSpec::vanilla(),
            InstanceOrigin::CinepolServer,
        );
        repo.save(&instance).await.unwrap();
        instance.record_playtime(60);
        repo.save(&instance).await.unwrap();
        assert_eq!(repo.list().await.unwrap(), vec![instance]);
        repo.delete(&id).await.unwrap();
        assert_eq!(
            repo.get(&id).await,
            Err(InstanceRepositoryError::NotFound(id.clone()))
        );
        assert_eq!(
            repo.delete(&id).await,
            Err(InstanceRepositoryError::NotFound(id))
        );
    }

    #[tokio::test]
    async fn server_list_store_starts_empty_per_instance() {
        let store = InMemoryServerListStore::default();
        let id = InstanceId::parse("a").unwrap();
        assert_eq!(store.load(&id).await.unwrap(), ServerList::default());
        let list = ServerList::new(vec![ServerEntry::new(
            "CINEPOL",
            ServerAddress::parse("mc.policarpiers.com").unwrap(),
        )]);
        store.save(&id, &list).await.unwrap();
        assert_eq!(store.load(&id).await.unwrap(), list);
        assert_eq!(
            store.load(&InstanceId::parse("b").unwrap()).await.unwrap(),
            ServerList::default()
        );
    }

    #[tokio::test]
    async fn java_probe_rejects_unknown_executables() {
        let version = JavaVersion {
            major: 21,
            full: "21.0.8".into(),
            is_64_bit: true,
        };
        let probe = FakeJavaProbe::with([(PathBuf::from("/jdk/bin/java"), version.clone())]);
        assert_eq!(probe.probe(Path::new("/jdk/bin/java")).await, Ok(version));
        assert!(matches!(
            probe.probe(Path::new("/nope")).await,
            Err(JavaProbeError::NotExecutable(_))
        ));
    }

    #[tokio::test]
    async fn loader_metadata_filters_by_game_version() {
        let release = LoaderRelease {
            version: LoaderVersion::parse("0.17.2").unwrap(),
            game_version: game("26.2"),
            stable: true,
        };
        let meta = FakeLoaderMetadata {
            kind: LoaderKind::Fabric,
            releases: vec![release.clone()],
        };
        assert_eq!(meta.releases(&game("26.2")).await, Ok(vec![release]));
        assert!(matches!(
            meta.releases(&game("1.8.9")).await,
            Err(LoaderMetadataError::UnsupportedGameVersion(
                LoaderKind::Fabric,
                _
            ))
        ));
    }

    #[tokio::test]
    async fn modpack_source_searches_titles_and_reports_unknown_projects() {
        let summary = ModpackSummary {
            source: ModpackSourceKind::Modrinth,
            project: "1KVo5zza".into(),
            title: "Fabulously Optimized".into(),
            description: String::new(),
            icon_url: None,
            downloads: Some(1),
        };
        let source = FakeModpackSource {
            kind: ModpackSourceKind::Modrinth,
            page: ModpackPage {
                items: vec![summary.clone()],
                total: Some(1),
            },
            versions: HashMap::new(),
        };
        let query = ModpackQuery {
            text: "fabulously".into(),
            ..Default::default()
        };
        assert_eq!(source.search(&query).await.unwrap().items, vec![summary]);
        let none = ModpackQuery {
            text: "skyblock".into(),
            ..Default::default()
        };
        assert!(source.search(&none).await.unwrap().items.is_empty());
        assert_eq!(
            source.versions("missing").await,
            Err(ModpackSourceError::NotFound("missing".into()))
        );
    }

    #[test]
    fn fixed_clock_only_moves_when_advanced() {
        let clock = FixedClock::at_unix(1_700_000_000);
        assert_eq!(
            crate::domain::ports::unix_seconds(clock.now()),
            1_700_000_000
        );
        clock.advance(Duration::from_secs(5));
        assert_eq!(
            crate::domain::ports::unix_seconds(clock.now()),
            1_700_000_005
        );
    }
}
