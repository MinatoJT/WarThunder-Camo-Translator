use camo_translator::catalog::is_game_root;
use std::ffi::OsString;
use std::fs;
use std::os::windows::ffi::OsStringExt;
use std::path::{Component, Path, PathBuf};
use std::ptr::null_mut;
use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::{
    HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RRF_SUBKEY_WOW6432KEY,
    RRF_SUBKEY_WOW6464KEY, RegGetValueW,
};

const UNINSTALL: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";
const STEAM: &str = r"SOFTWARE\Valve\Steam";
const GAME_FOLDERS: [&str; 2] = ["War Thunder", "WarThunder"];

pub(super) fn detect() -> Option<PathBuf> {
    let mut registered = ["Steam App 236390", "War Thunder", "WarThunder"]
        .into_iter()
        .flat_map(|game| registry_paths(&format!(r"{UNINSTALL}\{game}"), "InstallLocation"));
    if let Some(path) = registered.find(|path| is_game_root(path)) {
        return Some(path);
    }

    let program_folders = ["ProgramFiles(x86)", "ProgramFiles"]
        .into_iter()
        .filter_map(std::env::var_os)
        .map(PathBuf::from);
    let mut steam_folders = registry_paths(STEAM, "SteamPath")
        .into_iter()
        .chain(registry_paths(STEAM, "InstallPath"))
        .chain(program_folders.clone().map(|path| path.join("Steam")));
    if let Some(path) = steam_folders.find_map(|steam| in_steam_library(&steam)) {
        return Some(path);
    }

    program_folders
        .flat_map(|folder| GAME_FOLDERS.map(|name| folder.join(name)))
        .find(|path| is_game_root(path))
}

fn in_steam_library(steam: &Path) -> Option<PathBuf> {
    let library_file = steam.join("steamapps/libraryfolders.vdf");
    let description = fs::read_to_string(library_file).unwrap_or_default();
    let libraries = pairs(&description)
        .filter(|(key, _)| {
            key.eq_ignore_ascii_case("path") || key.bytes().all(|ch| ch.is_ascii_digit())
        })
        .map(|(_, value)| PathBuf::from(value.replace(r"\\", r"\")))
        .filter(|path| path.is_absolute());

    std::iter::once(steam.to_path_buf())
        .chain(libraries)
        .map(|library| library.join("steamapps"))
        .find_map(|apps| {
            let manifest =
                fs::read_to_string(apps.join("appmanifest_236390.acf")).unwrap_or_default();
            let folder = pairs(&manifest)
                .find(|(key, _)| key.eq_ignore_ascii_case("installdir"))
                .map(|(_, name)| name)
                .filter(|name| {
                    let mut parts = Path::new(name).components();
                    matches!(parts.next(), Some(Component::Normal(_))) && parts.next().is_none()
                });
            let common = apps.join("common");
            folder
                .into_iter()
                .chain(GAME_FOLDERS)
                .map(|name| common.join(name))
                .find(|path| is_game_root(path))
        })
}

fn pairs(text: &str) -> impl Iterator<Item = (&str, &str)> {
    text.trim_start_matches('\u{feff}')
        .lines()
        .filter_map(|line| {
            let (key, after_key) = line.trim_start().strip_prefix('"')?.split_once('"')?;
            let (value, _) = after_key.trim_start().strip_prefix('"')?.split_once('"')?;
            Some((key, value))
        })
}

fn registry_paths(key: &str, value: &str) -> Vec<PathBuf> {
    let key: Vec<u16> = key.encode_utf16().chain([0]).collect();
    let value: Vec<u16> = value.encode_utf16().chain([0]).collect();
    [
        (HKEY_CURRENT_USER, 0),
        (HKEY_LOCAL_MACHINE, RRF_SUBKEY_WOW6464KEY),
        (HKEY_LOCAL_MACHINE, RRF_SUBKEY_WOW6432KEY),
    ]
    .into_iter()
    .filter_map(|(root, view)| {
        let flags = view | RRF_RT_REG_SZ;
        let mut bytes = 0;
        let status = unsafe {
            RegGetValueW(
                root,
                key.as_ptr(),
                value.as_ptr(),
                flags,
                null_mut(),
                null_mut(),
                &mut bytes,
            )
        };
        if status != ERROR_SUCCESS || !(2..=65536).contains(&bytes) {
            return None;
        }
        let mut buffer = vec![0u16; (bytes as usize).div_ceil(2)];
        let status = unsafe {
            RegGetValueW(
                root,
                key.as_ptr(),
                value.as_ptr(),
                flags,
                null_mut(),
                buffer.as_mut_ptr().cast(),
                &mut bytes,
            )
        };
        if status != ERROR_SUCCESS {
            return None;
        }
        let len = buffer
            .iter()
            .position(|&ch| ch == 0)
            .unwrap_or(buffer.len());
        let path = PathBuf::from(OsString::from_wide(&buffer[..len]));
        path.is_absolute().then_some(path)
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovers_game_in_a_secondary_library() {
        let scratch = tempfile::tempdir().unwrap();
        let steam = scratch.path().join("Steam");
        let library = scratch.path().join("二号库 Steam Library");
        let game = library.join("steamapps/common/War Thunder Custom");
        let packs = game.join("patch/content/base/res/tanks");
        fs::create_dir_all(steam.join("steamapps")).unwrap();
        fs::create_dir_all(&packs).unwrap();
        let escaped = library.to_string_lossy().replace('\\', r"\\");
        let libraries =
            format!("\"libraryfolders\"\n{{\n\"1\"\n{{\n\"path\"\t\"{escaped}\"\n}}\n}}\n");
        fs::write(steam.join("steamapps/libraryfolders.vdf"), libraries).unwrap();
        fs::write(
            library.join("steamapps/appmanifest_236390.acf"),
            "\"AppState\"\n{\n\"installdir\"\t\"War Thunder Custom\"\n}\n",
        )
        .unwrap();
        fs::write(game.join("aces.vromfs.bin"), []).unwrap();

        assert_eq!(in_steam_library(&steam), Some(game));
    }

    #[test]
    #[ignore = "requires a locally registered War Thunder installation"]
    fn detects_the_local_game_installation() {
        let found = detect().expect("War Thunder installation");
        assert!(is_game_root(&found));
        println!("{}", found.display());
    }
}
