use std::path::Path;

use super::*;
use crate::domain::modpack::curseforge::{CurseForgePack, resolve_files};
use crate::domain::modpack::sync::EntrySource;
use crate::testing::FakeHttpClient;

const BASE: &str = "https://cf.test";
const KEY: &str = "test-key";
const FO: &str = "396246";

fn fixture(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/curseforge")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

fn source(bodies: Vec<(String, Vec<u8>)>) -> CurseForgeSource<FakeHttpClient> {
    CurseForgeSource::with_base_url(FakeHttpClient::with(bodies), Some(KEY), BASE)
}

fn search_url(rest: &str) -> String {
    format!(
        "{BASE}/v1/mods/search?gameId=432&classId=4471&searchFilter=fabulously%20optimized\
         &sortField=2&sortOrder=desc&index=20&pageSize=10{rest}"
    )
}

#[tokio::test]
async fn searches_modpacks_with_filters_and_the_api_key() {
    let url = search_url("&gameVersion=26.2&modLoaderType=4");
    let cf = source(vec![(url.clone(), fixture("search-modpacks.json"))]);
    let page = cf
        .search(&ModpackQuery {
            text: "fabulously optimized".into(),
            game_version: Some(GameVersion::parse("26.2").unwrap()),
            loader: Some(LoaderKind::Fabric),
            page: 2,
            page_size: 10,
        })
        .await
        .unwrap();

    assert_eq!(
        cf.http.keys(),
        [(url, "x-api-key".to_owned(), KEY.to_owned())]
    );
    let fo = page.items.iter().find(|item| item.project == FO).unwrap();
    assert_eq!(fo.source, ModpackSourceKind::CurseForge);
    assert_eq!(fo.title, "Fabulously Optimized");
    assert_eq!(fo.loaders, [LoaderKind::Fabric]);
    assert!(!fo.description.is_empty());
    assert!(fo.icon_url.as_deref().unwrap().starts_with("https://"));
    assert!(fo.downloads.unwrap() > 1_000_000);
    assert!(page.total.unwrap() >= page.items.len() as u64);
}

#[tokio::test]
async fn vanilla_searches_send_no_loader_filter() {
    let url = search_url("");
    let cf = source(vec![(url.clone(), fixture("search-modpacks.json"))]);
    cf.search(&ModpackQuery {
        text: "fabulously optimized".into(),
        game_version: None,
        loader: Some(LoaderKind::Vanilla),
        page: 2,
        page_size: 10,
    })
    .await
    .unwrap();
    assert_eq!(cf.http.requests(), [url]);
}

#[tokio::test]
async fn reads_a_modpack_and_its_installable_versions_newest_first() {
    let cf = source(vec![
        (format!("{BASE}/v1/mods/{FO}"), fixture("mod-modpack.json")),
        (
            format!("{BASE}/v1/mods/{FO}/files?pageSize=50"),
            fixture("modpack-files.json"),
        ),
    ]);
    let project = cf.project(FO).await.unwrap();
    assert_eq!(project.title, "Fabulously Optimized");
    assert_eq!(project.loaders, [LoaderKind::Fabric]);

    let versions = cf.versions(FO).await.unwrap();
    let ids: Vec<&str> = versions.iter().map(|v| v.id.as_str()).collect();
    assert_eq!(ids, ["9270300", "9127912", "9127669", "8872454", "8769267"]);
    let stable = &versions[1];
    assert_eq!(stable.name, "14.1.1 for 26.2");
    assert_eq!(stable.loaders, [LoaderKind::Fabric]);
    assert_eq!(stable.game_versions, [GameVersion::parse("26.2").unwrap()]);
    let file = stable.pack_file.as_ref().unwrap();
    assert_eq!(file.filename, "Fabulously.Optimized-14.1.1.zip");
    assert_eq!(file.size, 186_003);
    assert_eq!(file.checksum.algorithm(), HashAlgorithm::Sha1);
    assert!(file.url.starts_with("https://edge.forgecdn.net/"));
}

#[tokio::test]
async fn a_project_that_is_not_a_modpack_is_not_found() {
    let modpack = String::from_utf8(fixture("mod-modpack.json")).unwrap();
    let a_mod = modpack.replace("\"classId\":4471", "\"classId\":6");
    assert_ne!(a_mod, modpack);
    let cf = source(vec![(format!("{BASE}/v1/mods/{FO}"), a_mod.into_bytes())]);
    assert_eq!(
        cf.project(FO).await,
        Err(ModpackSourceError::NotFound(FO.into()))
    );
}

#[tokio::test]
async fn refuses_ids_that_are_not_numbers_without_asking() {
    let cf = source(vec![]);
    for bad in ["../x", "abc", "", "0", "1234567890123"] {
        assert_eq!(
            cf.project(bad).await,
            Err(ModpackSourceError::NotFound(bad.into())),
            "{bad:?}"
        );
    }
    assert!(cf.http.requests().is_empty());
}

#[tokio::test]
async fn looks_up_a_packs_files_and_projects_in_batches() {
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
    let files = cf.files(&[9100313, 1]).await.unwrap();
    let projects = cf.projects(&[448233]).await.unwrap();

    let posts = cf.http.posts();
    assert_eq!(posts[0].1, br#"{"fileIds":[9100313,1]}"#);
    assert_eq!(posts[1].1, br#"{"modIds":[448233]}"#);
    assert_eq!(cf.http.keys().len(), 2, "both batches carry the key");
    assert_eq!(files.len(), 51);
    let culling = files.iter().find(|f| f.id == 9100313).unwrap();
    assert_eq!(culling.project_id, 448233);
    assert_eq!(culling.download_url, None);
    assert_eq!(culling.sha1.as_deref().map(str::len), Some(40));
    assert_eq!(projects.len(), 51);
}

#[tokio::test]
async fn a_build_without_a_key_sends_nothing() {
    for key in [None, Some(""), Some("  ")] {
        let cf = CurseForgeSource::with_base_url(FakeHttpClient::default(), key, BASE);
        assert!(!cf.is_configured());
        let not_configured = Err(ModpackSourceError::NotConfigured(
            ModpackSourceKind::CurseForge,
        ));
        assert_eq!(cf.project(FO).await.map(|_| ()), not_configured);
        assert_eq!(cf.files(&[1]).await.map(|_| ()), not_configured);
        assert!(cf.http.requests().is_empty());
    }
}

#[tokio::test]
async fn a_refused_key_is_reported_as_such() {
    assert_eq!(
        source_error(
            HttpError::Status {
                url: format!("{BASE}/v1/mods/1"),
                status: 403
            },
            "1"
        ),
        ModpackSourceError::Network("CurseForge refused the launcher's API key".into())
    );
}

#[test]
fn the_real_pack_resolves_into_downloads_and_one_manual_file() {
    let pack =
        CurseForgePack::validate(parse_manifest(&fixture("manifest.json")).unwrap()).unwrap();
    assert_eq!(pack.game_version.as_str(), "26.2");
    assert_eq!(pack.loader.kind(), LoaderKind::Fabric);
    assert_eq!(pack.loader.version().unwrap().as_str(), "0.19.5");
    assert_eq!(pack.files.len(), 51);
    assert_eq!(pack.overrides, "overrides");

    let files: wire::Data<Vec<wire::File>> =
        serde_json::from_slice(&fixture("mods-files-batch.json")).unwrap();
    let projects: wire::Data<Vec<wire::Mod>> =
        serde_json::from_slice(&fixture("mods-batch-trimmed.json")).unwrap();
    let files: Vec<CurseForgeFile> = files.data.into_iter().map(file).collect();
    let projects: Vec<CurseForgeProject> = projects
        .data
        .into_iter()
        .map(|p| CurseForgeProject {
            id: p.id,
            name: p.name,
            class_id: p.class_id,
        })
        .collect();
    let resolved = resolve_files(&pack.files, &files, &projects).unwrap();

    assert_eq!(resolved.entries.len(), 50);
    assert_eq!(resolved.manual.len(), 1);
    assert_eq!(resolved.manual[0].name, "Entity Culling Fabric/Forge");
    assert_eq!(
        resolved.manual[0].path.as_str(),
        "mods/entityculling-fabric-1.11.3-mc26.2.jar"
    );
    let resourcepacks = resolved
        .entries
        .iter()
        .filter(|e| e.path.as_str().starts_with("resourcepacks/"))
        .count();
    assert_eq!(resourcepacks, 2);
    assert!(resolved.entries.iter().all(|e| matches!(
        &e.source,
        EntrySource::Download { checksum, size: Some(_), .. }
            if checksum.algorithm() == HashAlgorithm::Sha1
    )));
}

#[test]
fn rejects_a_broken_manifest() {
    assert!(matches!(
        parse_manifest(b"{\"minecraft\": 1}"),
        Err(PackFileError::InvalidArchive(_))
    ));
}

#[tokio::test]
async fn a_pack_file_off_the_curseforge_cdn_is_not_installable() {
    let body = String::from_utf8(fixture("modpack-files.json"))
        .unwrap()
        .replace("https://edge.forgecdn.net/", "https://evil.example/");
    let cf = source(vec![(
        format!("{BASE}/v1/mods/{FO}/files?pageSize=50"),
        body.into_bytes(),
    )]);
    let versions = cf.versions(FO).await.unwrap();
    assert_eq!(versions.len(), 5);
    assert!(versions.iter().all(|v| v.pack_file.is_none()));
}
