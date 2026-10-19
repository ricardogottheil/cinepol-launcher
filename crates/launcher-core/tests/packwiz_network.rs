//! Reads the real Fabulously Optimized 26.2 packwiz pack: `pack.toml`, its
//! verified index and two verified metafiles.
//!
//! Opt-in only: `cargo test -p cinepol-launcher-core --features network-tests --test packwiz_network`
#![cfg(feature = "network-tests")]
#![allow(clippy::expect_used, clippy::unwrap_used)]

use launcher_core::domain::launch::RelativePath;
use launcher_core::domain::modpack::packwiz::{
    Metafile, MetafileSource, PackwizIndex, PackwizPack,
};
use launcher_core::domain::modpack::{PackLocation, PackwizSource};
use launcher_core::infrastructure::http::ReqwestHttpClient;
use launcher_core::infrastructure::packwiz::PackwizFiles;

const USER_AGENT: &str =
    "ricardogottheil/cinepol-launcher/0.1.0 (https://cinepol.policarpiers.com)";

const PACK: &str = "https://raw.githubusercontent.com/Fabulously-Optimized/fabulously-optimized/main/Packwiz/26.2/pack.toml";

#[tokio::test]
async fn reads_the_fabulously_optimized_pack() {
    let source = PackwizFiles::new(ReqwestHttpClient::new(USER_AGENT).unwrap());
    let location = PackLocation::parse(PACK).unwrap();
    let pack = PackwizPack::validate(source.read_pack(&location).await.unwrap()).unwrap();
    assert_eq!(pack.name, "Fabulously Optimized");
    let raw = source
        .read_index(&location, &pack.index, &pack.index_checksum)
        .await
        .unwrap();
    let index = PackwizIndex::validate(raw).unwrap();
    assert!(index.entries.len() > 50, "{}", index.entries.len());

    for name in ["mods/fabric-api.pw.toml", "mods/sodium.pw.toml"] {
        let relative = RelativePath::parse(name).unwrap();
        let entry = index
            .entries
            .iter()
            .find(|entry| entry.path == relative)
            .unwrap_or_else(|| panic!("{name} is not in the index"));
        let raw = source
            .read_metafile(&location, &relative, &entry.checksum)
            .await
            .unwrap();
        let file = Metafile::validate(raw, &relative).unwrap().unwrap();
        assert!(
            matches!(&file.source, MetafileSource::Download { url } if url.starts_with("https://")),
            "{name}: {:?}",
            file.source
        );
    }
}
