// A switch as the user follows it: the adapter's dozen steps in five phases, and the sentences about how a job stands or how it ended. The words are the Electron app's, but for a game that was only handed to its launcher.

use ol_model::{
    JobState, JobStep, JobStepStatus, LaunchJob, LauncherId, RecoveryPhase, SwitchPlan,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PhaseId {
    Close,
    Switch,
    Start,
    SignIn,
    Launch,
}

impl PhaseId {
    pub const ORDER: [Self; 5] = [
        Self::Close,
        Self::Switch,
        Self::Start,
        Self::SignIn,
        Self::Launch,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Close => "close",
            Self::Switch => "switch",
            Self::Start => "start",
            Self::SignIn => "signin",
            Self::Launch => "launch",
        }
    }

    // A step no phase claims is part of the switch itself.
    pub fn of(step_id: &str) -> Self {
        match step_id {
            "close" => Self::Close,
            "start" | "wait" => Self::Start,
            "verify" | "login" | "manual" | "recapture" => Self::SignIn,
            "launch" => Self::Launch,
            _ => Self::Switch,
        }
    }
}

pub struct PhaseNames<'a> {
    // The program that is closed and started, as it reads in the middle of a sentence.
    pub launcher: &'a str,
    pub account: &'a str,
    pub game: Option<&'a str>,
}

pub fn phase_label(id: PhaseId, names: &PhaseNames<'_>) -> String {
    match id {
        PhaseId::Close => format!("Close {}", names.launcher),
        PhaseId::Switch => format!("Switch to {}", names.account),
        PhaseId::Start => format!("Start {}", names.launcher),
        PhaseId::SignIn => "Sign in".to_owned(),
        PhaseId::Launch => match names.game {
            Some(game) => format!("Launch {game}"),
            None => "Launch".to_owned(),
        },
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Phase<'a> {
    pub id: PhaseId,
    pub status: JobStepStatus,
    // The step that explains the phase: the one that runs, or the one that failed.
    pub current: Option<&'a JobStep>,
}

// A phase is done when its last step that ran is done, even after an earlier one failed (a sign-in by hand after the automatic one); it runs while a step runs and between two steps; it failed when the job ended while it was underway.
pub fn group_steps(steps: &[JobStep], job_running: bool) -> Vec<Phase<'_>> {
    let mut phases = Vec::new();
    for id in PhaseId::ORDER {
        let members: Vec<&JobStep> = steps
            .iter()
            .filter(|step| PhaseId::of(step.id) == id)
            .collect();
        if members.is_empty() {
            continue;
        }
        let live: Vec<&JobStep> = members
            .into_iter()
            .filter(|step| step.status != JobStepStatus::Skipped)
            .collect();
        let running = live
            .iter()
            .copied()
            .find(|step| step.status == JobStepStatus::Running);
        let last = live.last().copied();
        let failed = live
            .iter()
            .rev()
            .copied()
            .find(|step| step.status == JobStepStatus::Failed);
        let settled =
            |step: &&JobStep| matches!(step.status, JobStepStatus::Done | JobStepStatus::Failed);
        let mut status = match (running, last) {
            (Some(_), _) => JobStepStatus::Running,
            (None, None) => JobStepStatus::Skipped,
            (None, Some(last))
                if last.status == JobStepStatus::Done && live.iter().all(settled) =>
            {
                JobStepStatus::Done
            }
            (None, Some(last)) if last.status == JobStepStatus::Failed => JobStepStatus::Failed,
            (None, Some(_)) if live.iter().any(settled) => JobStepStatus::Running,
            (None, Some(_)) => JobStepStatus::Pending,
        };
        if status == JobStepStatus::Running && !job_running {
            status = JobStepStatus::Failed;
        }
        let current = running.or(if status == JobStepStatus::Failed {
            failed.or(last)
        } else {
            None
        });
        phases.push(Phase {
            id,
            status,
            current,
        });
    }
    phases
}

// Starts a sentence with a name that reads mid-sentence, such as "the Riot Client".
pub fn sentence(text: &str) -> String {
    let mut letters = text.chars();
    match letters.next() {
        // The Electron app raises one UTF-16 unit, so a first letter outside the basic plane stays as it is.
        Some(first) if u32::from(first) <= 0xffff => first.to_uppercase().chain(letters).collect(),
        _ => text.to_owned(),
    }
}

// The line under the sentence of a job that was stopped or that waits for a sign-in: the state the launcher was left in. Nothing when the job's own sentence says it all.
pub fn stopped_note(job: &LaunchJob, launcher: &str, action: &str) -> Option<String> {
    let client = sentence(launcher);
    if job.switched {
        return Some(format!(
            "{client} is signed in as {}. Press {action} again to launch the game.",
            job.account.display_name
        ));
    }
    match job.restored {
        Some(true) => {
            return Some(format!(
                "{client} was put back the way it was and left closed."
            ));
        }
        Some(false) => return Some(could_not_be_put_back(&client)),
        None => {}
    }
    let handed_over = job.steps.iter().any(|step| {
        step.id == "manual"
            && !matches!(step.status, JobStepStatus::Pending | JobStepStatus::Skipped)
    });
    if job.state == JobState::Cancelled && !handed_over {
        return None;
    }
    Some(format!(
        "Nothing was rolled back. Once {launcher} is signed in, press {action} again{}.",
        if job.game.is_some() {
            " to launch the game"
        } else {
            ""
        }
    ))
}

fn could_not_be_put_back(client: &str) -> String {
    format!(
        "{client} could not be put back automatically. Open Settings \u{203a} Backups and restore the latest snapshot."
    )
}

// The line under the error of a failed job: what was put back, and what was not.
pub fn failed_note(job: &LaunchJob, launcher: &str) -> String {
    let client = sentence(launcher);
    match job.restored {
        Some(true) => format!(
            "{client} was put back the way it was and left closed. Start it to check which account it signs in as."
        ),
        Some(false) => could_not_be_put_back(&client),
        None if job.switched => format!(
            "{client} is signed in as {}; only the game launch failed. Press Play again to retry the launch.",
            job.account.display_name
        ),
        None => "Nothing had been changed yet.".to_owned(),
    }
}

// The phases a plan would run, in order.
pub fn plan_phases(plan: &SwitchPlan, with_launch: bool) -> Vec<PhaseId> {
    PhaseId::ORDER
        .into_iter()
        .filter(|id| {
            (*id == PhaseId::Launch && with_launch)
                || plan
                    .steps
                    .iter()
                    .any(|step| !step.skipped && PhaseId::of(step.id) == *id)
        })
        .collect()
}

// A game that ended well, with no hand-over, whose start the launcher was not seen making: all One Launch knows is that the launcher was asked.
fn only_sent(job: &LaunchJob) -> bool {
    job.state == JobState::Done && job.game.is_some() && job.hand_off.is_none() && !job.game_seen
}

// The pill of a job that ended well.
pub fn done_pill(job: &LaunchJob) -> String {
    let account = &job.account.display_name;
    if job.game.is_none() || job.hand_off.is_some() {
        format!("Switched to {account}")
    } else if job.game_seen {
        format!("Launched as {account}")
    } else {
        format!("Sent to {} as {account}", job.launcher.client_name())
    }
}

// Under the pill of a game that was only handed to its launcher, which may update itself or the game before it starts it.
pub fn sent_note(job: &LaunchJob) -> Option<String> {
    let game = job.game.as_ref().filter(|_| only_sent(job))?;
    Some(format!(
        "{} starts {} from here. If it has to update itself or the game first, the game starts after that.",
        sentence(job.launcher.client_name()),
        game.name
    ))
}

// The one line of the banner that stands for a job while its dialog is closed.
pub fn banner_text(job: &LaunchJob) -> String {
    let account = &job.account.display_name;
    let game = job.game.as_ref().map(|game| game.name.as_str());
    match (job.state, game) {
        (JobState::Running, _) if job.attention.is_some() => {
            format!("Waiting for you to sign in as {account}")
        }
        (JobState::Running, Some(game)) if job.direct => {
            format!("Launching {game} as {account}\u{2026}")
        }
        (JobState::Running, Some(game)) => {
            format!("Switching to {account} and launching {game}\u{2026}")
        }
        (JobState::Running, None) => format!("Switching to {account}\u{2026}"),
        (JobState::Done, _) if job.hand_off.is_some() => format!(
            "Switched to {account}. {}",
            job.hand_off.as_deref().unwrap_or_default()
        ),
        (JobState::Done, Some(game)) if job.game_seen => format!("Launched {game} as {account}"),
        (JobState::Done, Some(game)) => format!(
            "Asked {} to start {game} as {account}",
            job.launcher.client_name()
        ),
        (JobState::Done, None) => format!("Switched to {account}"),
        (JobState::NeedsSignIn, _) => {
            format!("The switch to {account} is waiting for a sign-in")
        }
        (JobState::Cancelled, _) => format!("The switch to {account} was stopped"),
        (JobState::Failed, Some(game)) if job.switched => format!("Could not launch {game}"),
        (JobState::Failed, _) => format!("The switch to {account} failed"),
    }
}

// Said above the account list when the launcher wants to update the game first: Battle.net and the Riot Client will not start it until then, the others update it on the way.
pub fn update_note(launcher: LauncherId, game: &str) -> String {
    let (client, window) = match launcher {
        LauncherId::Battlenet => ("Battle.net", "the Battle.net window"),
        LauncherId::Riot => ("The Riot Client", "the Riot Client window"),
        LauncherId::Steam | LauncherId::Epic => {
            return format!(
                "{} updates {game} before it starts, so the launch may take a while.",
                sentence(launcher.client_name())
            );
        }
    };
    format!(
        "{client} wants to update {game} first. The switch still runs, but the game only starts once you update it in {window}."
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Recommend {
    Restore,
    Keep,
}

impl Recommend {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Restore => "restore",
            Self::Keep => "keep",
        }
    }
}

pub struct RecoveryCopy {
    pub title: &'static str,
    pub body: String,
    // The answer the dialog leads with.
    pub recommend: Recommend,
}

// What the app asks at start about a switch the last run did not finish.
pub fn recovery_copy(
    phase: RecoveryPhase,
    account_name: Option<&str>,
    snapshot_available: bool,
    launcher: &str,
) -> RecoveryCopy {
    let account = account_name.unwrap_or("an account that has since been removed");
    let title = if phase == RecoveryPhase::RollbackFailed {
        "The last switch could not be undone"
    } else {
        "One Launch was closed during a switch"
    };
    let (body, recommend) = if !snapshot_available {
        (
            format!(
                "It was switching {launcher} to {account}. The snapshot taken before that switch is gone, so there is nothing to restore from here. Start {launcher} and check which account it signs in as; Settings \u{203a} Backups lists the snapshots that are left."
            ),
            Recommend::Keep,
        )
    } else {
        match phase {
            RecoveryPhase::Writing => (
                format!(
                    "It stopped while changing {launcher}'s sign-in files for {account}, so {launcher} may be half-switched. Restoring puts {launcher} back the way it was before that switch."
                ),
                Recommend::Restore,
            ),
            RecoveryPhase::SigningIn => (
                format!(
                    "{launcher}'s files had already been changed for {account}, and {launcher} was starting or signing in. If it is signed in as {account}, keep it as it is. If it looks wrong, restore it to the way it was before the switch."
                ),
                Recommend::Keep,
            ),
            RecoveryPhase::RollbackFailed => (
                format!(
                    "The switch to {account} failed, and putting {launcher} back failed too. Restoring tries again with the snapshot taken before that switch."
                ),
                Recommend::Restore,
            ),
        }
    };
    RecoveryCopy {
        title,
        body,
        recommend,
    }
}
