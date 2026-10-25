#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;

use crate::domain::instance::GameVersion;
use crate::domain::integrity::HashAlgorithm;
use crate::domain::loader::LoaderKind;
use crate::domain::modpack::{ModpackSourceError, ModpackSourceKind};
use crate::domain::mods::{DependencyKind, ModQuery, ModSource, ModTarget, ReleaseChannel};
use crate::infrastructure::curseforge::CurseForgeSource;
use crate::testing::FakeHttpClient;

const BASE: &str = "https://cf.test";

fn fixture(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/curseforge")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

fn source(bodies: Vec<(String, Vec<u8>)>) -> CurseForgeSource<FakeHttpClient> {
    CurseForgeSource::with_base_url(FakeHttpClient::with(bodies), Some("test-key"), BASE)
}

fn target(loader: LoaderKind) -> ModTarget {
    ModTarget {
        game_version: GameVersion::parse("26.2").unwrap(),
        loader,
    }
}

/// One page of a project's files for one `modLoaderType`.
fn files_url(project: u64, loader: u8, index: u32) -> String {
    format!(
        "{BASE}/v1/mods/{project}/files?gameVersion=26.2&modLoaderType={loader}\
         &index={index}&pageSize=50"
    )
}

/// A Fabric jar of project 100 for 26.2, published at `date`.
fn file_json(id: u64, date: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "modId": 100,
        "fileName": format!("mod-{id}.jar"),
        "fileLength": 10,
        "hashes": [{ "value": "adac13c5fd8b82819830387cf9c8d8f405bd0ca5", "algo": 1 }],
        "downloadUrl": format!("https://edge.forgecdn.net/files/1/{id}/mod-{id}.jar"),
        "gameVersions": ["Fabric", "26.2"],
        "fileDate": date,
    })
}

fn files_page(files: Vec<serde_json::Value>, total: usize) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "data": files,
        "pagination": { "totalCount": total },
    }))
    .unwrap()
}

#[tokio::test]
async fn searches_mods_for_the_target_with_namespaced_ids() {
    let url = format!(
        "{BASE}/v1/mods/search?gameId=432&classId=6&searchFilter=fabric%20api\
         &sortField=2&sortOrder=desc&index=20&pageSize=10&gameVersion=26.2&modLoaderType=4"
    );
    let cf = source(vec![(url.clone(), fixture("search-mods.json"))]);
    let page = ModSource::search(
        &cf,
        &ModQuery {
            text: " fabric api ".into(),
            target: target(LoaderKind::Fabric),
            catalog: crate::domain::modpack::ModpackSourceKind::CurseForge,
            page: 2,
            page_size: 10,
        },
    )
    .await
    .unwrap();

    assert_eq!(cf.http.requests(), [url]);
    assert_eq!(page.total, Some(3));
    let fabric_api = &page.items[0];
    assert_eq!(fabric_api.project, "curseforge:306612");
    assert_eq!(fabric_api.title, "Fabric API");
    assert_eq!(fabric_api.downloads, Some(238_964_583));
    assert!(
        fabric_api
            .icon_url
            .as_deref()
            .unwrap()
            .starts_with("https://media.forgecdn.net/")
    );
}

#[tokio::test]
async fn nothing_is_asked_for_vanilla_or_without_a_key() {
    let cf = source(Vec::new());
    let query = ModQuery {
        text: String::new(),
        target: target(LoaderKind::Vanilla),
        catalog: crate::domain::modpack::ModpackSourceKind::CurseForge,
        page: 0,
        page_size: 10,
    };
    assert_eq!(ModSource::search(&cf, &query).await.unwrap().items, []);
    assert!(cf.http.requests().is_empty());

    let keyless = CurseForgeSource::with_base_url(FakeHttpClient::default(), None, BASE);
    let query = ModQuery {
        target: target(LoaderKind::Fabric),
        ..query
    };
    assert_eq!(
        ModSource::search(&keyless, &query).await.map(|_| ()),
        Err(ModpackSourceError::NotConfigured(
            ModpackSourceKind::CurseForge
        ))
    );
    assert!(keyless.http.requests().is_empty());
}

#[tokio::test]
async fn reads_a_projects_fitting_files_with_their_hash_and_dependencies() {
    // The fixture also lists other projects' files: they are never trusted.
    let cf = source(vec![(files_url(844662, 4, 0), fixture("mod-files.json"))]);
    let versions = ModSource::versions(&cf, "curseforge:844662", &target(LoaderKind::Fabric))
        .await
        .unwrap();

    assert_eq!(versions.len(), 1);
    let emf = &versions[0];
    assert_eq!(emf.id, "curseforge:9064962");
    assert_eq!(emf.project, "curseforge:844662");
    assert_eq!(emf.channel, ReleaseChannel::Release);
    assert_eq!(emf.loaders, [LoaderKind::Fabric, LoaderKind::Quilt]);
    assert_eq!(emf.manual_page, None);
    let file = emf.file.as_ref().unwrap();
    assert_eq!(
        file.filename,
        "entity_model_features-3.3.11-26.2-fabric.jar"
    );
    assert!(file.url.starts_with("https://edge.forgecdn.net/"));
    assert_eq!(file.size, 664_593);
    assert_eq!(file.checksum.algorithm(), HashAlgorithm::Sha1);
    assert_eq!(
        file.checksum.value(),
        "adac13c5fd8b82819830387cf9c8d8f405bd0ca5"
    );
    assert_eq!(emf.dependencies.len(), 1);
    assert_eq!(
        emf.dependencies[0].project.as_deref(),
        Some("curseforge:568563")
    );
    assert_eq!(emf.dependencies[0].kind, DependencyKind::Required);
}

#[tokio::test]
async fn maps_release_types_and_every_relation_it_understands() {
    // A Quilt instance also runs Fabric files: both loaders are asked, and a
    // file tagged with both is listed once.
    let cf = source(vec![
        (files_url(619986, 5, 0), fixture("mod-files.json")),
        (files_url(619986, 4, 0), fixture("mod-files.json")),
    ]);
    let versions = ModSource::versions(&cf, "curseforge:619986", &target(LoaderKind::Quilt))
        .await
        .unwrap();

    assert_eq!(
        cf.http.requests(),
        [files_url(619986, 5, 0), files_url(619986, 4, 0)]
    );
    assert_eq!(versions.len(), 1);

    let optigui = &versions[0];
    assert_eq!(optigui.channel, ReleaseChannel::Beta);
    let kinds: Vec<(Option<&str>, DependencyKind)> = optigui
        .dependencies
        .iter()
        .map(|dependency| (dependency.project.as_deref(), dependency.kind))
        .collect();
    assert_eq!(
        kinds,
        [
            (Some("curseforge:533006"), DependencyKind::Optional),
            (Some("curseforge:306612"), DependencyKind::Required),
            (Some("curseforge:308769"), DependencyKind::Required),
        ]
    );
    assert!(
        optigui
            .dependencies
            .iter()
            .all(|dependency| dependency.version.is_none())
    );
}

#[tokio::test]
async fn a_file_its_author_keeps_off_launchers_points_to_its_page_instead() {
    let cf = source(vec![(files_url(448233, 4, 0), fixture("mod-files.json"))]);
    let versions = ModSource::versions(&cf, "curseforge:448233", &target(LoaderKind::Fabric))
        .await
        .unwrap();

    let culling = &versions[0];
    assert_eq!(culling.file, None);
    assert_eq!(
        culling.manual_page.as_deref(),
        Some("https://www.curseforge.com/projects/448233")
    );
}

#[tokio::test]
async fn reads_one_file_and_a_batch_of_projects_by_their_ids() {
    let cf = source(vec![
        (
            format!("{BASE}/v1/mods/files"),
            fixture("mods-files-batch.json"),
        ),
        (
            format!("{BASE}/v1/mods"),
            fixture("mods-batch-trimmed.json"),
        ),
    ]);
    let emf = ModSource::version(&cf, "curseforge:9064962").await.unwrap();
    let projects = ModSource::projects(
        &cf,
        &[
            "curseforge:306612".into(),
            "curseforge:308702".into(),
            "AANobbMI".into(),
        ],
    )
    .await
    .unwrap();

    let posts = cf.http.posts();
    assert_eq!(posts[0].1, br#"{"fileIds":[9064962]}"#);
    assert_eq!(posts[1].1, br#"{"modIds":[306612,308702]}"#);
    assert_eq!(emf.project, "curseforge:844662");
    // The answer lists more projects than were asked for; only those come back.
    let titles: Vec<&str> = projects
        .iter()
        .map(|project| project.title.as_str())
        .collect();
    assert_eq!(titles, ["Fabric API", "Mod Menu"]);
}

#[tokio::test]
async fn ids_from_another_catalog_are_refused_before_any_request() {
    let cf = source(Vec::new());
    for id in ["AANobbMI", "306612", "curseforge:abc", "curseforge:0"] {
        assert_eq!(
            ModSource::versions(&cf, id, &target(LoaderKind::Fabric))
                .await
                .map(|_| ()),
            Err(ModpackSourceError::NotFound(id.into())),
            "{id}"
        );
        assert_eq!(
            ModSource::version(&cf, id).await.map(|_| ()),
            Err(ModpackSourceError::NotFound(id.into())),
            "{id}"
        );
    }
    assert!(
        ModSource::projects(&cf, &["AANobbMI".into()])
            .await
            .unwrap()
            .is_empty()
    );
    // Loose jars are only identified by Modrinth.
    assert!(
        ModSource::identify(&cf, &["a".repeat(128)])
            .await
            .unwrap()
            .is_empty()
    );
    assert!(cf.http.requests().is_empty());
}

#[tokio::test]
async fn walks_every_page_of_a_loaders_files_to_find_the_newest() {
    // 51 files: the newest is alone on the second page.
    let old: Vec<serde_json::Value> = (1..=50)
        .map(|id| file_json(id, &format!("2025-01-{:02}T10:00:00Z", id % 28 + 1)))
        .collect();
    let cf = source(vec![
        (files_url(100, 4, 0), files_page(old, 51)),
        (
            files_url(100, 4, 50),
            files_page(vec![file_json(51, "2026-10-01T10:00:00Z")], 51),
        ),
    ]);
    let versions = ModSource::versions(&cf, "curseforge:100", &target(LoaderKind::Fabric))
        .await
        .unwrap();

    assert_eq!(
        cf.http.requests(),
        [files_url(100, 4, 0), files_url(100, 4, 50)]
    );
    assert_eq!(versions.len(), 51);
    assert_eq!(versions[0].id, "curseforge:51");
}

#[tokio::test]
async fn orders_files_by_their_moment_however_the_date_is_written() {
    // Same second; CurseForge drops the fraction when it is zero.
    let cf = source(vec![(
        files_url(100, 4, 0),
        files_page(
            vec![
                file_json(1, "2026-10-05T00:51:17Z"),
                file_json(2, "2026-10-05T00:51:17.5Z"),
                file_json(3, "2026-10-05T00:51:17.25Z"),
            ],
            3,
        ),
    )]);
    let versions = ModSource::versions(&cf, "curseforge:100", &target(LoaderKind::Fabric))
        .await
        .unwrap();

    let ids: Vec<&str> = versions.iter().map(|version| version.id.as_str()).collect();
    assert_eq!(ids, ["curseforge:2", "curseforge:3", "curseforge:1"]);
}
