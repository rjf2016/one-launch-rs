use ol_formats::winpath;
use ol_model::InstalledGame;
use ol_platform_api::{FileReader, ProcessInfo};

// The first game one of these processes runs out of. A process path is where the image really is, so a game's folder counts as recorded and with links followed. Lettered drives only: a launcher file must not send the app to a share it names.
pub fn game_with_running_process<'g>(
    files: &dyn FileReader,
    processes: &[ProcessInfo],
    games: &'g [InstalledGame],
) -> Option<&'g InstalledGame> {
    let paths: Vec<String> = processes
        .iter()
        .filter_map(|process| process.path.as_deref())
        .filter(|path| !path.is_empty())
        .map(str::to_lowercase)
        .collect();
    if paths.is_empty() {
        return None;
    }
    let folders: Vec<(&InstalledGame, Vec<String>)> = games
        .iter()
        .map(|game| {
            let lettered = winpath::is_absolute(&game.install_path)
                && game.install_path.as_bytes().get(1) == Some(&b':');
            let resolved = lettered
                .then(|| files.resolved(&game.install_path))
                .flatten();
            let prefixes = std::iter::once(game.install_path.clone())
                .chain(resolved)
                .map(|folder| format!("{}\\", folder.to_lowercase()))
                .collect();
            (game, prefixes)
        })
        .collect();
    paths.iter().find_map(|path| {
        folders
            .iter()
            .find(|(_, prefixes)| prefixes.iter().any(|prefix| path.starts_with(prefix)))
            .map(|(game, _)| *game)
    })
}
