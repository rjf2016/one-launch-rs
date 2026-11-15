#![forbid(unsafe_code)]

// Every case of oracle/l1 for what the launch dialog and the recovery dialog say, run against the port. A case is left out only for a reason named here.

use ol_model::{
    JobAccount, JobGame, JobState, JobStep, JobStepStatus, LaunchJob, LauncherId, PlanAccount,
    RecoveryPhase, SwitchPlan, SwitchPlanStep, SwitchStepKind, SwitchStrategy,
};
use ol_services::phases::{
    PhaseId, PhaseNames, group_steps, phase_label, plan_phases, recovery_copy, sentence,
    stopped_note,
};
use ol_testkit::vectors::{Answer, Case, UNPAIRED, run};
use serde_json::{Value, json};

const PROTOTYPE: &str = "a step id that names a member of JavaScript's Object.prototype, which no adapter sends and which is no phase";
const NO_PHASE: &str = "an id that is no phase, which the Rust type rules out";
const NO_RECOVERY_PHASE: &str = "a phase the journal never gives, which the Rust type rules out";

fn text(value: &Value) -> String {
    value.as_str().expect("text").to_owned()
}

// A step id is a constant of the adapter in the app; a vector's lives as long as the test.
fn step(value: &Value) -> JobStep {
    let status = match value["status"].as_str().expect("a status") {
        "pending" => JobStepStatus::Pending,
        "running" => JobStepStatus::Running,
        "done" => JobStepStatus::Done,
        "skipped" => JobStepStatus::Skipped,
        "failed" => JobStepStatus::Failed,
        other => panic!("no such status: {other}"),
    };
    JobStep {
        id: text(&value["id"]).leak(),
        title: text(&value["title"]),
        status,
        detail: text(&value["detail"]),
    }
}

fn steps(value: &Value) -> Vec<JobStep> {
    value.as_array().expect("steps").iter().map(step).collect()
}

fn step_json(step: &JobStep) -> Value {
    json!({ "id": step.id, "status": step.status.as_str(), "title": step.title, "detail": step.detail })
}

fn job(value: &Value) -> LaunchJob {
    let state = match value["state"].as_str().expect("a state") {
        "running" => JobState::Running,
        "done" => JobState::Done,
        "failed" => JobState::Failed,
        "needs-sign-in" => JobState::NeedsSignIn,
        "cancelled" => JobState::Cancelled,
        other => panic!("no such state: {other}"),
    };
    let account = &value["account"];
    LaunchJob {
        id: "job".to_owned(),
        state,
        launcher: LauncherId::Steam,
        account: JobAccount {
            id: text(&account["id"]),
            display_name: text(&account["displayName"]),
            login: text(&account["login"]),
        },
        game: value["game"].as_object().map(|game| JobGame {
            id: text(&game["id"]),
            name: text(&game["name"]),
            launcher_game_id: text(&game["launcherGameId"]),
        }),
        steps: steps(&value["steps"]),
        attention: None,
        direct: false,
        switched: value["switched"] == true,
        hand_off: None,
        backup_id: None,
        restored: value["restored"].as_bool(),
        error: None,
        started_at: String::new(),
        finished_at: None,
    }
}

fn plan(value: &Value) -> SwitchPlan {
    let steps = value["steps"]
        .as_array()
        .expect("steps")
        .iter()
        .map(|step| SwitchPlanStep {
            id: text(&step["id"]).leak(),
            kind: SwitchStepKind::Wait,
            title: String::new(),
            target: String::new(),
            detail: String::new(),
            changes: Vec::new(),
            skipped: step["skipped"] == true,
        })
        .collect();
    SwitchPlan {
        launcher: LauncherId::Steam,
        account: PlanAccount {
            id: None,
            display_name: String::new(),
            login: String::new(),
            launcher_uid: None,
        },
        strategy: SwitchStrategy::Remembered,
        most_recent_login: None,
        auto_login_user: None,
        running: false,
        steps,
        warnings: Vec::new(),
        notes: Vec::new(),
        executable: true,
        generated_at: String::new(),
    }
}

fn names_prototype_member(id: &str) -> bool {
    [
        "constructor",
        "toString",
        "__proto__",
        "hasOwnProperty",
        "valueOf",
    ]
    .contains(&id)
}

fn any_step_names_prototype_member(steps: &Value) -> bool {
    steps
        .as_array()
        .expect("steps")
        .iter()
        .any(|step| names_prototype_member(step["id"].as_str().unwrap_or_default()))
}

fn phase(id: &str) -> Option<PhaseId> {
    PhaseId::ORDER
        .into_iter()
        .find(|phase| phase.as_str() == id)
}

fn skipped<'a>(cases: &[&'a str], reason: &'a str) -> Vec<(&'a str, &'a str)> {
    cases.iter().map(|case| (*case, reason)).collect()
}

#[test]
fn a_step_belongs_to_the_phase_the_electron_app_puts_it_in() {
    let mut left_out = vec![("lone-surrogate", UNPAIRED)];
    left_out.extend(skipped(
        &[
            "proto-constructor",
            "proto-tostring",
            "proto-proto",
            "proto-hasownproperty",
        ],
        PROTOTYPE,
    ));
    run(
        "l1/core/renderer.phases.phaseOf.json",
        &left_out,
        |case: &Case| {
            let id = text(&case.args[0]);
            if names_prototype_member(&id) {
                return Answer::Skip(PROTOTYPE);
            }
            Answer::Out(PhaseId::of(&id).as_str().into())
        },
    );
}

#[test]
fn a_phase_is_named_for_the_user() {
    run(
        "l1/core/renderer.phases.phaseLabel.json",
        &[("unknown-id", NO_PHASE)],
        |case: &Case| {
            let Some(id) = phase(&text(&case.args[0])) else {
                return Answer::Skip(NO_PHASE);
            };
            let names = &case.args[1];
            let (launcher, account) = (text(&names["launcher"]), text(&names["account"]));
            let game = names["game"].as_str();
            Answer::Out(
                phase_label(
                    id,
                    &PhaseNames {
                        launcher: &launcher,
                        account: &account,
                        // The Electron app takes an empty name for no game.
                        game: game.filter(|game| !game.is_empty()),
                    },
                )
                .into(),
            )
        },
    );
}

#[test]
fn steps_fall_into_phases_with_the_state_the_electron_app_gives_each() {
    run(
        "l1/core/renderer.phases.groupSteps.json",
        &[("proto-step-id", PROTOTYPE)],
        |case: &Case| {
            if any_step_names_prototype_member(&case.args[0]) {
                return Answer::Skip(PROTOTYPE);
            }
            let steps = steps(&case.args[0]);
            let phases = group_steps(&steps, case.args[1] == true);
            Answer::Out(
                phases
                    .iter()
                    .map(|phase| {
                        json!({
                            "id": phase.id.as_str(),
                            "status": phase.status.as_str(),
                            "current": phase.current.map(step_json),
                        })
                    })
                    .collect(),
            )
        },
    );
}

#[test]
fn a_name_starts_a_sentence() {
    run(
        "l1/core/renderer.phases.sentence.json",
        &[("lone-surrogate", UNPAIRED)],
        |case: &Case| Answer::Out(sentence(&text(&case.args[0])).into()),
    );
}

#[test]
fn a_stopped_job_says_what_state_the_launcher_was_left_in() {
    run(
        "l1/core/renderer.phases.stoppedNote.json",
        &[],
        |case: &Case| {
            let job = job(&case.args[0]);
            let note = stopped_note(&job, &text(&case.args[1]), &text(&case.args[2]));
            Answer::Out(note.map_or(Value::Null, Value::String))
        },
    );
}

#[test]
fn a_plan_lists_the_phases_it_would_run() {
    run(
        "l1/core/renderer.phases.planPhases.json",
        &[("proto-id", PROTOTYPE)],
        |case: &Case| {
            if any_step_names_prototype_member(&case.args[0]["steps"]) {
                return Answer::Skip(PROTOTYPE);
            }
            let phases = plan_phases(&plan(&case.args[0]), case.args[1] == true);
            Answer::Out(phases.iter().map(|phase| phase.as_str()).collect())
        },
    );
}

#[test]
fn the_recovery_prompt_has_the_electron_apps_words() {
    run(
        "l1/core/renderer.recovery.recoveryCopy.json",
        &skipped(
            &["unknown-phase-snapshot", "unknown-phase-no-snapshot"],
            NO_RECOVERY_PHASE,
        ),
        |case: &Case| {
            let recovery = &case.args[0];
            let phase = match recovery["phase"].as_str() {
                Some("writing") => RecoveryPhase::Writing,
                Some("signing-in") => RecoveryPhase::SigningIn,
                Some("rollback-failed") => RecoveryPhase::RollbackFailed,
                _ => return Answer::Skip(NO_RECOVERY_PHASE),
            };
            let copy = recovery_copy(
                phase,
                recovery["accountName"].as_str(),
                recovery["snapshotAvailable"] == true,
                &text(&case.args[1]),
            );
            Answer::Out(json!({
                "title": copy.title,
                "body": copy.body,
                "recommend": copy.recommend.as_str(),
            }))
        },
    );
}
