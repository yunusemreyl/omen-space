use std::path::Path;

pub fn get_asset_path(filename: &str) -> String {
    // Prefer the workspace assets directory during local development so newly added
    // or updated icons aren't shadowed by an older system installation.
    let manifest_path = format!("{}/assets/{}", env!("CARGO_MANIFEST_DIR"), filename);
    if Path::new(&manifest_path).exists() {
        return manifest_path;
    }

    let local_path = format!("assets/{}", filename);
    if Path::new(&local_path).exists() {
        return local_path;
    }

    format!("/usr/share/omen-space/assets/{}", filename)
}
