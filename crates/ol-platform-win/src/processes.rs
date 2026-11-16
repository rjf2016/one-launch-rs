// The process list, a close by window, an end, and a start, from the kernel's own process table: no process is opened to list it, to name its file or to close its windows, and one is opened only to end it. Which process may be touched is decided here, in plain functions; the calls to Windows sit behind `System`, so a made-up table can show that a Riot game, Vanguard and whatever they started are never chosen. Windows answers by id and by window handle, which it hands out again: one handed to such a process between the last read of the table and the call is the window that is left.

mod command_line;
mod win32;

use std::collections::{BTreeSet, HashMap, HashSet};

use ol_platform_api::protected::{image_key, is_protected};
use ol_platform_api::{ProcessArg, ProcessError, ProcessInfo, ProcessLister, ProcessRunner};

use self::command_line::{Unwritable, command_line};
use self::win32::Win32;

// One row of the kernel's table: its image name and who started it, never more.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Entry {
    pub pid: u32,
    pub parent: u32,
    // When it started, in 100 ns ticks since 1601: with the id, it names one process, as an id alone does not once Windows hands it out again.
    pub created: u64,
    // The Windows sign-in it runs in: another user's processes are in the table too.
    pub session: u32,
    pub name: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct TopWindow {
    pub handle: isize,
    pub pid: u32,
    pub owned: bool,
    pub visible: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Posted {
    Yes,
    Refused,
    Failed,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ended {
    Yes,
    Gone,
    Refused,
    Failed(u32),
}

// What the port asks of Windows. Only `end` opens a process, and only `create` makes one.
pub(crate) trait System: Send + Sync {
    fn own_pid(&self) -> u32;
    fn snapshot(&self) -> Result<Vec<Entry>, ProcessError>;
    fn top_windows(&self) -> Vec<TopWindow>;
    fn post_close(&self, window: isize) -> Posted;
    // Each target is ended only if the process that has its id now started when the target did.
    fn end(&self, targets: &[Entry]) -> Vec<Ended>;
    fn image_paths(&self, pids: &[u32]) -> Vec<Option<String>>;
    fn create(&self, exe: &str, command_line: &mut [u16], cwd: &str) -> Result<(), u32>;
}

fn index(entries: &[Entry]) -> HashMap<u32, &Entry> {
    entries.iter().map(|entry| (entry.pid, entry)).collect()
}

// A parent id counts only when that process is older than the child: Windows hands out the ids of ended processes again.
fn parent_of<'a>(child: &Entry, by_pid: &HashMap<u32, &'a Entry>) -> Option<&'a Entry> {
    by_pid
        .get(&child.parent)
        .copied()
        .filter(|parent| parent.pid != child.pid && parent.created <= child.created)
}

// Whoever has the parent's id now, however old: for the shield alone, which a clock set back must not cut short and which may take in too much.
fn any_parent_of<'a>(child: &Entry, by_pid: &HashMap<u32, &'a Entry>) -> Option<&'a Entry> {
    by_pid
        .get(&child.parent)
        .copied()
        .filter(|parent| parent.pid != child.pid)
}

fn lineage<'a>(
    entry: &'a Entry,
    by_pid: &HashMap<u32, &'a Entry>,
    parent: impl Fn(&Entry, &HashMap<u32, &'a Entry>) -> Option<&'a Entry>,
) -> Vec<&'a Entry> {
    let mut line = vec![entry];
    let mut at = entry;
    while let Some(up) = parent(at, by_pid) {
        if line.iter().any(|seen| seen.pid == up.pid) || line.len() > by_pid.len() {
            break;
        }
        line.push(up);
        at = up;
    }
    line
}

// The protected processes and everything they started, at any depth.
pub(crate) fn shielded(entries: &[Entry]) -> HashSet<u32> {
    let by_pid = index(entries);
    entries
        .iter()
        .filter(|entry| {
            lineage(entry, &by_pid, any_parent_of)
                .iter()
                .any(|step| is_protected(&step.name))
        })
        .map(|entry| entry.pid)
        .collect()
}

// The processes with a shielded one anywhere below them.
fn above_shielded(entries: &[Entry], shielded: &HashSet<u32>) -> HashSet<u32> {
    let by_pid = index(entries);
    entries
        .iter()
        .filter(|entry| shielded.contains(&entry.pid))
        .flat_map(|entry| lineage(entry, &by_pid, any_parent_of).into_iter().skip(1))
        .map(|step| step.pid)
        .filter(|pid| !shielded.contains(pid))
        .collect()
}

// The processes this app descends from, not counting itself.
fn ancestors(entries: &[Entry], own_pid: u32) -> HashSet<u32> {
    let by_pid = index(entries);
    by_pid
        .get(&own_pid)
        .map(|own| {
            lineage(own, &by_pid, parent_of)
                .iter()
                .skip(1)
                .map(|step| step.pid)
                .collect()
        })
        .unwrap_or_default()
}

fn named(entry: &Entry, image: &str) -> bool {
    image_key(&entry.name) == image_key(image)
}

fn same_process(entry: &Entry, now: &Entry) -> bool {
    now.pid == entry.pid && now.created == entry.created && now.name == entry.name
}

fn session_of(entries: &[Entry], own_pid: u32) -> Option<u32> {
    entries
        .iter()
        .find(|entry| entry.pid == own_pid)
        .map(|entry| entry.session)
}

// A close or an end acts only in this app's own Windows session, and refuses while the image also runs in another: the launcher would still count as running.
fn in_other_session(entries: &[Entry], image: &str, own_pid: u32) -> Result<(), ProcessError> {
    let own = session_of(entries, own_pid);
    if entries
        .iter()
        .any(|entry| named(entry, image) && Some(entry.session) != own)
    {
        return Err(ProcessError::Failed(format!(
            "{image} is running under another Windows user on this PC. One Launch closes programs only for the user it runs as: close it in that user's session, then try again."
        )));
    }
    Ok(())
}

// The processes a kill ends, the ones they started first. Only of this app's session; never a protected process or one of its descendants, never this app or what it descends from, and below the named image nothing with a protected process under it, nor what that started.
pub(crate) fn end_targets(entries: &[Entry], image: &str, tree: bool, own_pid: u32) -> Vec<Entry> {
    let by_pid = index(entries);
    let shielded = shielded(entries);
    let above = above_shielded(entries, &shielded);
    let session = session_of(entries, own_pid);
    let mut spared = ancestors(entries, own_pid);
    spared.insert(own_pid);
    let untouchable = |entry: &Entry| {
        shielded.contains(&entry.pid)
            || spared.contains(&entry.pid)
            || Some(entry.session) != session
    };

    let mut targets = Vec::new();
    let mut visited = HashSet::new();
    for root in entries.iter().filter(|entry| named(entry, image)) {
        if untouchable(root) {
            continue;
        }
        let mut stack = vec![(root, false)];
        while let Some((node, expanded)) = stack.pop() {
            if expanded {
                targets.push(node.clone());
                continue;
            }
            if !visited.insert(node.pid) {
                continue;
            }
            stack.push((node, true));
            if tree {
                let children = entries.iter().filter(|child| {
                    !untouchable(child)
                        && !above.contains(&child.pid)
                        && parent_of(child, &by_pid).is_some_and(|parent| parent.pid == node.pid)
                });
                stack.extend(children.map(|child| (child, false)));
            }
        }
    }
    targets
}

// For each process of the image that was there before and after the windows were listed, the same process both times: its first visible window without an owner, as taskkill picks it, or else every hidden one without an owner.
pub(crate) fn close_targets(
    before: &[Entry],
    windows: &[TopWindow],
    after: &[Entry],
    image: &str,
    own_pid: u32,
) -> Vec<Vec<isize>> {
    let (shielded_before, shielded_after) = (shielded(before), shielded(after));
    let session = session_of(before, own_pid);
    let after_by_pid = index(after);
    before
        .iter()
        .filter(|entry| named(entry, image) && entry.pid != own_pid)
        .filter(|entry| Some(entry.session) == session)
        .filter(|entry| !shielded_before.contains(&entry.pid))
        .filter(|entry| {
            after_by_pid.get(&entry.pid).is_some_and(|same| {
                same.created == entry.created
                    && same.session == entry.session
                    && named(same, image)
                    && !shielded_after.contains(&same.pid)
            })
        })
        .map(|entry| {
            let mut chosen = Vec::new();
            for window in windows
                .iter()
                .filter(|window| window.pid == entry.pid && !window.owned)
            {
                if window.visible {
                    chosen.clear();
                    chosen.push(window.handle);
                    break;
                }
                chosen.push(window.handle);
            }
            chosen
        })
        .collect()
}

// The processes whose file may be named: all but the protected ones and what they started.
pub(crate) fn path_wanted(entries: &[Entry]) -> Vec<bool> {
    let shielded = shielded(entries);
    entries
        .iter()
        .map(|entry| !shielded.contains(&entry.pid))
        .collect()
}

fn checked(image: &str) -> Result<(), ProcessError> {
    let plain =
        !image.trim().is_empty() && image.len() <= 260 && !image.contains(['\\', '/', ':', '\0']);
    if !plain {
        return Err(ProcessError::Failed(
            "One Launch was asked about a process by something that is not an image name"
                .to_owned(),
        ));
    }
    if is_protected(image) {
        return Err(refused(image));
    }
    Ok(())
}

fn refused(image: &str) -> ProcessError {
    ProcessError::Failed(format!(
        "One Launch never starts, closes or ends {image}: it is a Riot game or part of Vanguard"
    ))
}

// On a lettered drive or a share: Windows looks for any other path in folders of its own choosing.
fn full_path(exe: &str) -> bool {
    let separator = |byte: u8| byte == b'\\' || byte == b'/';
    if let [letter, b':', after, rest @ ..] = exe.as_bytes() {
        return letter.is_ascii_alphabetic() && separator(*after) && !rest.is_empty();
    }
    let Some(share) = exe.strip_prefix(r"\\") else {
        return false;
    };
    let mut parts = share.splitn(3, '\\');
    let (Some(server), Some(name), Some(file)) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    !matches!(server, "" | "." | "?") && !name.is_empty() && !file.is_empty()
}

fn image_of(exe: &str) -> &str {
    exe.rsplit(['\\', '/']).next().unwrap_or(exe)
}

// Windows drops a name's trailing dots and spaces, takes `name:stream` for the file, and follows a short name or a link to its file: each spelling is checked, and the file it lands on.
fn start_refused(exe: &str) -> bool {
    let given = image_of(exe);
    let landed = std::fs::canonicalize(exe).ok().and_then(|real| {
        real.file_name()
            .map(|name| name.to_string_lossy().into_owned())
    });
    given.contains(':')
        || is_protected(given.trim_end_matches(['.', ' ']))
        || landed.is_some_and(|name| is_protected(&name))
}

#[derive(Default)]
pub struct WindowsProcesses(Port<Win32>);

impl WindowsProcesses {
    pub const fn new() -> Self {
        Self(Port { system: Win32 })
    }
}

impl ProcessLister for WindowsProcesses {
    fn is_running(&self, image: &str) -> Result<bool, ProcessError> {
        self.0.is_running(image)
    }

    fn list_processes(&self) -> Result<Vec<ProcessInfo>, ProcessError> {
        self.0.list_processes()
    }

    fn running_names(&self) -> Result<Vec<String>, ProcessError> {
        self.0.running_names()
    }
}

impl ProcessRunner for WindowsProcesses {
    fn start(&self, exe: &str, args: &[ProcessArg<'_>], cwd: &str) -> Result<(), ProcessError> {
        self.0.start(exe, args, cwd)
    }

    fn close(&self, image: &str) -> Result<(), ProcessError> {
        self.0.close(image)
    }

    fn kill(&self, image: &str, tree: bool) -> Result<(), ProcessError> {
        self.0.kill(image, tree)
    }
}

#[derive(Default)]
pub(crate) struct Port<S> {
    system: S,
}

impl<S: System> ProcessLister for Port<S> {
    fn is_running(&self, image: &str) -> Result<bool, ProcessError> {
        let entries = self.system.snapshot()?;
        Ok(entries
            .iter()
            .any(|entry| !entry.name.is_empty() && named(entry, image)))
    }

    // A path is asked by id: the table is read again just before, so an id handed to another process since is not asked, and just after, so a path that may be another process's is dropped.
    fn list_processes(&self) -> Result<Vec<ProcessInfo>, ProcessError> {
        let entries = self.system.snapshot()?;
        let wanted = path_wanted(&entries);
        let named: Vec<(Entry, bool)> = entries
            .into_iter()
            .zip(wanted)
            .filter(|(entry, _)| !entry.name.is_empty())
            .collect();
        let before = self.system.snapshot()?;
        let still_wanted = path_wanted(&before);
        let before_by_pid: HashMap<u32, (&Entry, bool)> = before
            .iter()
            .zip(still_wanted)
            .map(|(entry, wanted)| (entry.pid, (entry, wanted)))
            .collect();
        let asked: Vec<&Entry> = named
            .iter()
            .filter(|(entry, wanted)| {
                *wanted
                    && before_by_pid
                        .get(&entry.pid)
                        .is_some_and(|(now, wanted)| *wanted && same_process(entry, now))
            })
            .map(|(entry, _)| entry)
            .collect();
        let pids: Vec<u32> = asked.iter().map(|entry| entry.pid).collect();
        let found = self.system.image_paths(&pids);
        let after = self.system.snapshot()?;
        let after_by_pid = index(&after);
        let mut paths: HashMap<u32, String> = asked
            .iter()
            .zip(found)
            .filter_map(|(entry, path)| {
                let kept = after_by_pid
                    .get(&entry.pid)
                    .is_some_and(|now| same_process(entry, now));
                Some((entry.pid, path.filter(|_| kept)?))
            })
            .collect();
        Ok(named
            .into_iter()
            .map(|(entry, _)| ProcessInfo {
                path: paths.remove(&entry.pid),
                name: entry.name,
                pid: entry.pid,
            })
            .collect())
    }

    fn running_names(&self) -> Result<Vec<String>, ProcessError> {
        let entries = self.system.snapshot()?;
        let names: BTreeSet<String> = entries
            .iter()
            .filter(|entry| !entry.name.is_empty())
            .map(|entry| entry.name.to_lowercase())
            .collect();
        Ok(names.into_iter().collect())
    }
}

impl<S: System> ProcessRunner for Port<S> {
    fn start(&self, exe: &str, args: &[ProcessArg<'_>], cwd: &str) -> Result<(), ProcessError> {
        // Windows reads a path only up to a NUL, and the name checks below would read past it.
        if exe.contains('\0') || cwd.contains('\0') {
            return Err(ProcessError::Failed(
                "One Launch was asked to start a program by a path or a folder that holds a NUL"
                    .to_owned(),
            ));
        }
        let image = image_of(exe);
        if !full_path(exe) {
            return Err(ProcessError::Failed(format!(
                "Could not start {image}: One Launch starts a program only by its full path"
            )));
        }
        if start_refused(exe) {
            return Err(refused(image));
        }
        let mut line = command_line(exe, args).map_err(|unwritable| {
            ProcessError::Failed(match unwritable {
                Unwritable::QuotedProgram => {
                    format!("Could not start {image}: its path holds a quote")
                }
                Unwritable::Nul => format!("Could not start {image}: an argument holds a NUL"),
            })
        })?;
        self.system.create(exe, &mut line, cwd).map_err(|code| {
            ProcessError::Failed(format!("Could not start {image} (Windows error {code})"))
        })
    }

    fn close(&self, image: &str) -> Result<(), ProcessError> {
        checked(image)?;
        let own_pid = self.system.own_pid();
        let before = self.system.snapshot()?;
        in_other_session(&before, image, own_pid)?;
        let windows = self.system.top_windows();
        let after = self.system.snapshot()?;
        let mut denied = false;
        for windows in close_targets(&before, &windows, &after, image, own_pid) {
            let mut posted = false;
            let mut refused = false;
            for window in windows {
                match self.system.post_close(window) {
                    Posted::Yes => posted = true,
                    Posted::Refused => refused = true,
                    Posted::Failed => {}
                }
            }
            denied |= refused && !posted;
        }
        if denied {
            return Err(ProcessError::AccessDenied(image.to_owned()));
        }
        Ok(())
    }

    fn kill(&self, image: &str, tree: bool) -> Result<(), ProcessError> {
        checked(image)?;
        let entries = self.system.snapshot()?;
        let own_pid = self.system.own_pid();
        in_other_session(&entries, image, own_pid)?;
        let ancestors = ancestors(&entries, own_pid);
        if entries
            .iter()
            .any(|entry| named(entry, image) && ancestors.contains(&entry.pid))
        {
            return Err(ProcessError::Failed(format!(
                "One Launch was started from {image}, so it will not end it. Close {image} yourself, or start One Launch on its own, then try again."
            )));
        }
        let chosen = end_targets(&entries, image, tree, own_pid);
        if chosen.is_empty() {
            return Ok(());
        }
        // Chosen again from a table read just before the opens: an id handed to another process since, or a process that now has a Riot game below it, is left.
        let still = end_targets(&self.system.snapshot()?, image, tree, own_pid);
        let targets: Vec<Entry> = chosen
            .into_iter()
            .filter(|target| still.contains(target))
            .collect();
        let ended = self.system.end(&targets);
        let (mut denied, mut failed) = (false, None);
        for (target, ended) in targets.iter().zip(ended) {
            if !named(target, image) {
                continue;
            }
            match ended {
                Ended::Refused => denied = true,
                Ended::Failed(code) => failed = Some(code),
                Ended::Yes | Ended::Gone => {}
            }
        }
        if denied {
            return Err(ProcessError::AccessDenied(image.to_owned()));
        }
        if let Some(code) = failed {
            return Err(ProcessError::Failed(format!(
                "Could not end {image} (Windows error {code})"
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use ol_platform_api::protected::PROTECTED_IMAGES;

    use super::*;

    const OWN: u32 = 900;

    const SESSION: u32 = 1;

    fn entry(pid: u32, parent: u32, created: u64, name: &str) -> Entry {
        in_session(pid, parent, created, name, SESSION)
    }

    fn in_session(pid: u32, parent: u32, created: u64, name: &str, session: u32) -> Entry {
        Entry {
            pid,
            parent,
            created,
            session,
            name: name.to_owned(),
        }
    }

    // A PC with a Riot client and its game, Vanguard, Steam with a helper and a game, and this app under explorer; a reused id in it and a process started by the game.
    fn table() -> Vec<Entry> {
        vec![
            entry(4, 0, 1, "System"),
            entry(100, 4, 2, "services.exe"),
            entry(110, 100, 3, "vgc.exe"),
            entry(200, 4, 2, "explorer.exe"),
            entry(210, 200, 5, "vgtray.exe"),
            entry(OWN, 200, 9, "OneLaunch.exe"),
            entry(300, 200, 10, "RiotClientServices.exe"),
            entry(310, 300, 11, "Riot Client.exe"),
            entry(320, 300, 12, "VALORANT.exe"),
            entry(321, 320, 13, "VALORANT-Win64-Shipping.exe"),
            entry(322, 321, 14, "CrashReporter.exe"),
            entry(330, 300, 15, "RiotClientServices.exe"),
            entry(400, 200, 20, "steam.exe"),
            entry(410, 400, 21, "steamwebhelper.exe"),
            entry(411, 410, 22, "steamwebhelper.exe"),
            entry(420, 400, 23, "Game.exe"),
            // Started by a process with id 400 that ended long before this steam.exe took the id.
            entry(430, 400, 19, "Unrelated.exe"),
            entry(500, 322, 30, "steam.exe"),
        ]
    }

    #[derive(Default)]
    struct Calls {
        ended: Vec<u32>,
        posted: Vec<isize>,
        pathed: Vec<u32>,
        created: Vec<String>,
        reads: usize,
    }

    struct MadeUp {
        entries: Vec<Entry>,
        // The tables the reads after the first one see, in turn; the last one stays.
        then: Vec<Vec<Entry>>,
        windows: Vec<TopWindow>,
        calls: Mutex<Calls>,
        refuse: bool,
    }

    impl MadeUp {
        fn new(entries: Vec<Entry>) -> Self {
            let windows = entries
                .iter()
                .flat_map(|entry| {
                    let handle = isize::try_from(entry.pid).unwrap() * 10;
                    [
                        TopWindow {
                            handle,
                            pid: entry.pid,
                            owned: false,
                            visible: false,
                        },
                        TopWindow {
                            handle: handle + 1,
                            pid: entry.pid,
                            owned: false,
                            visible: true,
                        },
                        TopWindow {
                            handle: handle + 2,
                            pid: entry.pid,
                            owned: true,
                            visible: true,
                        },
                    ]
                })
                .collect();
            Self {
                entries,
                then: Vec::new(),
                windows,
                calls: Mutex::default(),
                refuse: false,
            }
        }

        fn calls(&self) -> std::sync::MutexGuard<'_, Calls> {
            self.calls.lock().unwrap()
        }
    }

    impl System for MadeUp {
        fn own_pid(&self) -> u32 {
            OWN
        }

        fn snapshot(&self) -> Result<Vec<Entry>, ProcessError> {
            let mut calls = self.calls();
            calls.reads += 1;
            let later = calls.reads.checked_sub(2).map(|at| {
                self.then
                    .get(at)
                    .or(self.then.last())
                    .unwrap_or(&self.entries)
            });
            Ok(later.unwrap_or(&self.entries).clone())
        }

        fn top_windows(&self) -> Vec<TopWindow> {
            self.windows.clone()
        }

        fn post_close(&self, window: isize) -> Posted {
            self.calls().posted.push(window);
            if self.refuse {
                Posted::Refused
            } else {
                Posted::Yes
            }
        }

        fn end(&self, targets: &[Entry]) -> Vec<Ended> {
            self.calls()
                .ended
                .extend(targets.iter().map(|target| target.pid));
            targets
                .iter()
                .map(|_| {
                    if self.refuse {
                        Ended::Refused
                    } else {
                        Ended::Yes
                    }
                })
                .collect()
        }

        fn image_paths(&self, pids: &[u32]) -> Vec<Option<String>> {
            self.calls().pathed.extend(pids);
            pids.iter()
                .map(|pid| Some(format!(r"D:\made-up\{pid}.exe")))
                .collect()
        }

        fn create(&self, exe: &str, _: &mut [u16], _: &str) -> Result<(), u32> {
            self.calls().created.push(exe.to_owned());
            Err(2)
        }
    }

    fn protected_pids() -> HashSet<u32> {
        HashSet::from([110, 210, 320, 321, 322, 500])
    }

    fn touched(calls: &Calls) -> HashSet<u32> {
        calls
            .ended
            .iter()
            .chain(&calls.pathed)
            .copied()
            .chain(
                calls
                    .posted
                    .iter()
                    .map(|window| u32::try_from(window / 10).unwrap()),
            )
            .collect()
    }

    #[test]
    fn the_shield_covers_riot_games_vanguard_and_whatever_they_started() {
        assert_eq!(shielded(&table()), protected_pids());
    }

    #[test]
    fn a_tree_end_takes_what_the_image_started_and_nothing_shielded_or_unrelated() {
        let pids =
            |targets: Vec<Entry>| -> Vec<u32> { targets.iter().map(|target| target.pid).collect() };
        let steam = pids(end_targets(&table(), "STEAM", true, OWN));
        assert_eq!(
            steam.iter().copied().collect::<HashSet<_>>(),
            HashSet::from([400, 410, 411, 420])
        );
        assert!(
            steam.iter().position(|pid| *pid == 411) < steam.iter().position(|pid| *pid == 410)
        );
        assert_eq!(steam.last(), Some(&400));
        assert_eq!(pids(end_targets(&table(), "steam.exe", false, OWN)), [400]);

        let client = pids(end_targets(&table(), "RiotClientServices.exe", true, OWN));
        assert_eq!(
            client.iter().copied().collect::<HashSet<_>>(),
            HashSet::from([300, 310, 330])
        );
        assert!(pids(end_targets(&table(), "explorer.exe", true, OWN)).is_empty());
        assert!(pids(end_targets(&table(), "OneLaunch.exe", true, OWN)).is_empty());
        for image in PROTECTED_IMAGES {
            assert!(
                end_targets(&table(), image, true, OWN).is_empty(),
                "{image}"
            );
        }
    }

    #[test]
    fn a_tree_end_spares_whatever_has_a_riot_game_below_it_but_still_ends_the_named_image() {
        let table = vec![
            entry(4, 0, 1, "System"),
            entry(200, 4, 2, "explorer.exe"),
            entry(OWN, 200, 3, "OneLaunch.exe"),
            entry(400, 200, 10, "steam.exe"),
            entry(401, 400, 11, "RiotClientServices.exe"),
            entry(402, 401, 12, "VALORANT.exe"),
            entry(403, 402, 13, "VALORANT-Win64-Shipping.exe"),
            entry(404, 401, 14, "Riot Client.exe"),
            entry(405, 404, 15, "RiotClientCrashHandler.exe"),
            entry(410, 400, 16, "steamwebhelper.exe"),
            entry(411, 410, 17, "steamwebhelper.exe"),
            entry(420, 400, 18, "Game.exe"),
        ];
        let ended: HashSet<u32> = end_targets(&table, "steam.exe", true, OWN)
            .iter()
            .map(|target| target.pid)
            .collect();
        assert_eq!(ended, HashSet::from([400, 410, 411, 420]));

        let processes = Port {
            system: MadeUp::new(table),
        };
        processes.kill("steam.exe", true).unwrap();
        let calls = processes.system.calls();
        assert_eq!(
            calls.ended.iter().copied().collect::<HashSet<_>>(),
            HashSet::from([400, 410, 411, 420])
        );
    }

    #[test]
    fn an_end_of_the_program_this_app_was_started_from_says_why_and_ends_nothing() {
        let table = vec![
            entry(200, 4, 2, "explorer.exe"),
            entry(400, 200, 10, "steam.exe"),
            entry(410, 400, 11, "steamwebhelper.exe"),
            entry(OWN, 400, 12, "OneLaunch.exe"),
        ];
        let processes = Port {
            system: MadeUp::new(table),
        };
        for tree in [true, false] {
            let refused = processes.kill("STEAM.exe", tree).unwrap_err().to_string();
            assert_eq!(
                refused,
                "One Launch was started from STEAM.exe, so it will not end it. Close STEAM.exe yourself, or start One Launch on its own, then try again."
            );
        }
        assert!(processes.kill("steamwebhelper.exe", false).is_ok());
        let calls = processes.system.calls();
        assert_eq!(calls.ended, [410]);
    }

    #[test]
    fn another_windows_users_programs_are_seen_and_never_closed_or_ended() {
        let table = vec![
            in_session(200, 4, 2, "explorer.exe", SESSION),
            in_session(OWN, 200, 3, "OneLaunch.exe", SESSION),
            in_session(250, 4, 2, "explorer.exe", 2),
            in_session(400, 250, 10, "steam.exe", 2),
            in_session(410, 400, 11, "steamwebhelper.exe", 2),
            in_session(500, 200, 12, "EpicGamesLauncher.exe", SESSION),
            in_session(510, 250, 13, "EpicGamesLauncher.exe", 2),
            in_session(600, 200, 14, "Battle.net.exe", SESSION),
            in_session(610, 600, 15, "Agent.exe", 2),
            in_session(620, 600, 16, "Battle.net Helper.exe", SESSION),
        ];
        let processes = Port {
            system: MadeUp::new(table.clone()),
        };
        assert!(processes.is_running("steam.exe").unwrap());
        assert!(
            processes
                .running_names()
                .unwrap()
                .contains(&"steam.exe".to_owned())
        );
        assert_eq!(processes.list_processes().unwrap().len(), table.len());

        for image in ["steam.exe", "EpicGamesLauncher.exe"] {
            let wanted = format!(
                "{image} is running under another Windows user on this PC. One Launch closes programs only for the user it runs as: close it in that user's session, then try again."
            );
            for refused in [
                processes.kill(image, true),
                processes.kill(image, false),
                processes.close(image),
            ] {
                assert_eq!(refused.unwrap_err(), ProcessError::Failed(wanted.clone()));
            }
        }
        {
            let calls = processes.system.calls();
            assert!(calls.ended.is_empty() && calls.posted.is_empty());
        }
        processes.kill("Battle.net.exe", true).unwrap();
        processes.close("Battle.net.exe").unwrap();
        let calls = processes.system.calls();
        assert_eq!(
            calls.ended.iter().copied().collect::<HashSet<_>>(),
            HashSet::from([600, 620])
        );
        assert_eq!(calls.posted, [6001]);
        assert!(
            end_targets(&table, "Agent.exe", true, OWN).is_empty(),
            "a process of another session is no target"
        );
        let windows = MadeUp::new(table.clone()).windows;
        assert!(close_targets(&table, &windows, &table, "Agent.exe", OWN).is_empty());
    }

    #[test]
    fn an_id_handed_to_another_process_before_the_open_is_not_opened() {
        let first = vec![
            entry(200, 4, 2, "explorer.exe"),
            entry(OWN, 200, 3, "OneLaunch.exe"),
            entry(400, 200, 10, "steam.exe"),
            entry(410, 400, 11, "steamwebhelper.exe"),
            entry(420, 400, 12, "Game.exe"),
            entry(430, 400, 13, "Overlay.exe"),
        ];
        let mut taken = first.clone();
        taken[2] = entry(400, 200, 50, "VALORANT.exe");
        let mut gone = first.clone();
        gone.retain(|entry| entry.pid != 420);
        let mut riot_below = first.clone();
        riot_below.push(entry(431, 430, 14, "LeagueClient.exe"));

        let ended = |then: Vec<Entry>, image: &str| {
            let mut made_up = MadeUp::new(first.clone());
            made_up.then = vec![then];
            let processes = Port { system: made_up };
            let _ = processes.kill(image, true);
            let calls = processes.system.calls();
            calls.ended.iter().copied().collect::<HashSet<_>>()
        };
        assert_eq!(
            ended(first.clone(), "steam.exe"),
            HashSet::from([400, 410, 420, 430])
        );
        assert!(ended(taken, "steam.exe").is_empty());
        assert_eq!(ended(gone, "steam.exe"), HashSet::from([400, 410, 430]));
        assert_eq!(
            ended(riot_below, "steam.exe"),
            HashSet::from([400, 410, 420])
        );
    }

    #[test]
    fn a_path_is_asked_only_of_a_process_that_stayed_the_same_and_kept_only_while_it_does() {
        let first = table();
        let mut taken = first.clone();
        let reused = taken.iter_mut().find(|entry| entry.pid == 410).unwrap();
        *reused = entry(410, 300, 60, "VALORANT.exe");
        let mut renewed = taken.clone();
        renewed
            .iter_mut()
            .find(|entry| entry.pid == 420)
            .unwrap()
            .created = 70;

        let mut made_up = MadeUp::new(first);
        made_up.then = vec![taken, renewed];
        let processes = Port { system: made_up };
        let listed = processes.list_processes().unwrap();
        let path = |pid: u32| {
            listed
                .iter()
                .find(|process| process.pid == pid)
                .unwrap()
                .path
                .clone()
        };
        assert!(!processes.system.calls().pathed.contains(&410));
        assert_eq!(path(410), None);
        assert_eq!(path(420), None);
        assert_eq!(path(400).as_deref(), Some(r"D:\made-up\400.exe"));
        assert_eq!(path(200).as_deref(), Some(r"D:\made-up\200.exe"));
    }

    #[test]
    fn a_child_of_a_riot_game_stays_shielded_after_the_clock_was_set_back() {
        let mut table = table();
        table.push(entry(323, 320, 4, "Overlay.exe"));
        table.push(entry(324, 323, 5, "Helper.exe"));
        let shield = shielded(&table);
        assert!(shield.contains(&323) && shield.contains(&324));
        assert!(end_targets(&table, "Overlay.exe", true, OWN).is_empty());
        assert!(end_targets(&table, "Helper.exe", false, OWN).is_empty());
        let client: HashSet<u32> = end_targets(&table, "RiotClientServices.exe", true, OWN)
            .iter()
            .map(|target| target.pid)
            .collect();
        assert_eq!(client, HashSet::from([300, 310, 330]));
    }

    #[test]
    fn a_close_goes_to_the_visible_window_and_only_of_a_process_that_stayed_the_same() {
        let made_up = MadeUp::new(table());
        assert_eq!(
            close_targets(&table(), &made_up.windows, &table(), "Riot Client", OWN),
            [vec![3101]]
        );
        let hidden = [
            TopWindow {
                handle: 7,
                pid: 400,
                owned: false,
                visible: false,
            },
            TopWindow {
                handle: 8,
                pid: 400,
                owned: false,
                visible: false,
            },
        ];
        assert_eq!(
            close_targets(&table(), &hidden, &table(), "steam.exe", OWN),
            [vec![7, 8]]
        );

        let mut after = table();
        after
            .iter_mut()
            .find(|entry| entry.pid == 400)
            .unwrap()
            .created = 99;
        assert!(close_targets(&table(), &hidden, &after, "steam.exe", OWN).is_empty());
        let mut taken = table();
        let reused = taken.iter_mut().find(|entry| entry.pid == 400).unwrap();
        reused.name = "VALORANT.exe".to_owned();
        assert!(close_targets(&table(), &hidden, &taken, "steam.exe", OWN).is_empty());
        assert!(close_targets(&taken, &hidden, &table(), "steam.exe", OWN).is_empty());
    }

    #[test]
    fn whatever_the_caller_asks_no_protected_process_is_opened_named_or_sent_a_close() {
        let processes = Port {
            system: MadeUp::new(table()),
        };
        let mut images: Vec<String> = table().iter().map(|entry| entry.name.clone()).collect();
        images.extend(PROTECTED_IMAGES.iter().map(|image| (*image).to_owned()));
        images.extend(["valorant", "VGC", "", " ", r"C:\x\vgc.exe", "*"].map(str::to_owned));
        for image in &images {
            let _ = processes.close(image);
            let _ = processes.kill(image, true);
            let _ = processes.kill(image, false);
            let _ = processes.is_running(image);
        }
        let listed = processes.list_processes().unwrap();
        let _ = processes.running_names();

        let calls = processes.system.calls();
        assert!(!calls.ended.is_empty() && !calls.posted.is_empty() && !calls.pathed.is_empty());
        let touched = touched(&calls);
        for pid in protected_pids() {
            assert!(!touched.contains(&pid), "process {pid} was touched");
        }
        assert!(!calls.ended.contains(&OWN) && !calls.ended.contains(&200));
        assert!(!calls.posted.iter().any(|window| window / 10 == 900));
        for process in &listed {
            assert_eq!(
                process.path.is_none(),
                protected_pids().contains(&process.pid),
                "{}",
                process.pid
            );
        }
        assert_eq!(listed.len(), table().len());
    }

    #[test]
    fn a_protected_image_is_refused_before_anything_is_asked() {
        let processes = Port {
            system: MadeUp::new(table()),
        };
        for image in ["VALORANT.exe", "vgtray", "League of Legends.exe"] {
            let closed = processes.close(image).unwrap_err().to_string();
            assert!(closed.contains("Riot game or part of Vanguard"), "{closed}");
            assert!(processes.kill(image, false).is_err());
        }
        for exe in [
            r"D:\Riot Games\VALORANT\live\VALORANT.exe",
            r"D:\Riot Games\VALORANT\live\valorant",
            r"D:\Riot Games\VALORANT\live\VALORANT.exe. .",
            r"D:\Riot Games\VALORANT\live\VALORANT.exe::$DATA",
            r"D:/Riot Vanguard/vgtray.exe",
        ] {
            let started = processes.start(exe, &[], "").unwrap_err().to_string();
            assert!(
                started.contains("Riot game or part of Vanguard"),
                "{started}"
            );
        }
        let calls = processes.system.calls();
        assert!(calls.ended.is_empty() && calls.posted.is_empty() && calls.created.is_empty());
    }

    #[test]
    fn a_start_with_a_nul_or_without_a_full_path_is_refused_before_anything_else() {
        let processes = Port {
            system: MadeUp::new(table()),
        };
        for (exe, cwd) in [
            (
                "D:\\Riot Games\\VALORANT\\live\\VALORANT.exe\0\\steam.exe",
                "",
            ),
            ("D:\\Steam\\steam.exe\0", ""),
            (r"D:\Steam\steam.exe", "D:\\Steam\0"),
        ] {
            let refused = processes.start(exe, &[], cwd).unwrap_err().to_string();
            assert_eq!(
                refused,
                "One Launch was asked to start a program by a path or a folder that holds a NUL"
            );
        }
        for exe in [
            "steam.exe",
            r"Steam\steam.exe",
            r"\Steam\steam.exe",
            "D:steam.exe",
            r"\\?\D:\Steam\steam.exe",
            r"\\.\D:\Steam\steam.exe",
            r"\\server",
            r"\\server\\steam.exe",
            "",
        ] {
            let refused = processes.start(exe, &[], "").unwrap_err().to_string();
            assert!(
                refused.ends_with("One Launch starts a program only by its full path"),
                "{exe}: {refused}"
            );
        }
        for exe in [
            r"D:\Steam\steam.exe",
            "d:/steam/steam.exe",
            r"\\server\share\steam.exe",
        ] {
            let failed = processes.start(exe, &[], "").unwrap_err().to_string();
            assert!(failed.contains("Windows error 2"), "{exe}: {failed}");
        }
        assert_eq!(
            processes.system.calls().created,
            [
                r"D:\Steam\steam.exe",
                "d:/steam/steam.exe",
                r"\\server\share\steam.exe"
            ]
        );
    }

    #[test]
    fn windows_refusing_a_close_or_an_end_is_access_denied() {
        let mut made_up = MadeUp::new(table());
        made_up.refuse = true;
        let processes = Port { system: made_up };
        let closed = processes.close("steam.exe").unwrap_err();
        assert_eq!(closed.code(), Some("PROCESS_ACCESS_DENIED"));
        let ended = processes.kill("steam.exe", true).unwrap_err();
        assert_eq!(ended, ProcessError::AccessDenied("steam.exe".to_owned()));
        assert!(processes.kill("nothing-runs-this.exe", true).is_ok());
    }

    #[test]
    fn a_secret_argument_is_in_no_error() {
        let processes = Port {
            system: MadeUp::new(table()),
        };
        for args in [
            &[
                ProcessArg::Plain("-login"),
                ProcessArg::Secret("placeholder-password-1"),
            ][..],
            &[ProcessArg::Secret("placeholder\0password")],
        ] {
            let refused = processes
                .start(r"D:\Steam\steam.exe", args, r"D:\Steam")
                .unwrap_err();
            let shown = format!("{refused} {refused:?}");
            assert!(!shown.contains("placeholder"), "{shown}");
            assert!(shown.contains("steam.exe"), "{shown}");
        }
    }

    #[test]
    fn the_names_are_every_image_in_lower_case_once() {
        let processes = Port {
            system: MadeUp::new(table()),
        };
        let names = processes.running_names().unwrap();
        assert!(names.contains(&"valorant-win64-shipping.exe".to_owned()));
        assert_eq!(names.iter().filter(|name| *name == "steam.exe").count(), 1);
        assert!(processes.is_running("VALORANT").unwrap());
        assert!(!processes.is_running("nothing.exe").unwrap());
    }
}
