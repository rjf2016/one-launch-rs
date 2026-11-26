// The launch dialog as it is drawn: pick an account, see what the switch would do, confirm closing the launcher, follow the job and read how it ended.

use std::time::SystemTime;

use gpui::prelude::*;
use gpui::{
    AnyElement, App, BoxShadow, ClickEvent, Context, Div, FontWeight, HighlightStyle, Hsla,
    SharedString, Stateful, StyledText, div, hsla, px,
};
use ol_api::clipboard::CopyField;
use ol_api::launch::{
    PhaseNames, done_pill, failed_note, group_steps, phase_label, plan_phases, sent_note, sentence,
    stopped_note, update_note, waits_for_you,
};
use ol_api::library::{date_time, relative};
use ol_model::{
    Account, JobState, JobStepStatus, LaunchAttention, LaunchJob, LaunchPreflight, LauncherId,
    SwitchPlan, SwitchStepKind, SwitchStrategy,
};
use ol_ui_kit::theme::{self, alpha, color};

use super::{Accounts, LaunchHost, Request, Stage, can_import};
use crate::assets;
use crate::local_time;
use crate::widgets::{Button, PillTone, Tone, icon, launcher_badge, pill, spinner, tip};

#[derive(Clone, Copy)]
enum Paint {
    Warning,
    Danger,
    Plain,
    Quiet,
}

fn framed(paint: Paint) -> Div {
    let (line, fill, text): (Hsla, Hsla, u32) = match paint {
        Paint::Warning => (
            alpha(theme::WARNING, 0.3),
            alpha(theme::WARNING, 0.1),
            theme::WARNING,
        ),
        Paint::Danger => (
            alpha(theme::DANGER, 0.3),
            alpha(theme::DANGER, 0.1),
            theme::DANGER,
        ),
        Paint::Plain => (
            color(theme::BORDER).into(),
            alpha(theme::SURFACE_2, 0.6),
            theme::TEXT,
        ),
        Paint::Quiet => (
            color(theme::BORDER).into(),
            alpha(theme::SURFACE_2, 0.6),
            theme::MUTED,
        ),
    };
    div()
        .rounded(px(8.))
        .border_1()
        .border_color(line)
        .bg(fill)
        .px(px(12.))
        .py(px(10.))
        .text_color(color(text))
}

fn small(element: Div) -> Div {
    element.text_size(px(12.)).line_height(px(16.))
}

fn tiny(element: Div) -> Div {
    element.text_size(px(11.)).line_height(px(15.))
}

// A mark and the words beside it, wrapped under each other and never under the mark.
fn marked(glyph: &'static str, size: f32, hex: u32, words: impl IntoElement) -> Div {
    div()
        .flex()
        .items_start()
        .gap(px(8.))
        .child(icon(glyph, size, hex).mt(px(2.)))
        .child(div().flex_1().min_w_0().child(words))
}

fn step_mark(status: JobStepStatus, id: (&'static str, usize)) -> AnyElement {
    match status {
        JobStepStatus::Done => icon(assets::CHECK, 16., theme::SUCCESS).into_any_element(),
        JobStepStatus::Running => spinner(id, 16., theme::ACCENT),
        JobStepStatus::Failed => icon(assets::FAILED, 16., theme::DANGER).into_any_element(),
        JobStepStatus::Pending | JobStepStatus::Skipped => {
            icon(assets::DASHED, 16., theme::FAINT).into_any_element()
        }
    }
}

fn waits(status: JobStepStatus) -> bool {
    matches!(status, JobStepStatus::Pending | JobStepStatus::Skipped)
}

// Whatever a hand-over step puts in quotes is the one thing to tick or press, so it stands out.
fn quoted_bold(step: &str) -> StyledText {
    let mut bold = Vec::new();
    let mut opened: Option<(usize, char)> = None;
    for (at, letter) in step.char_indices() {
        match (opened, letter) {
            (None, '\u{201c}' | '"') => opened = Some((at, letter)),
            (Some((from, '\u{201c}')), '\u{201d}') | (Some((from, '"')), '"') => {
                bold.push((
                    from..at + letter.len_utf8(),
                    HighlightStyle {
                        color: Some(color(theme::TEXT_STRONG).into()),
                        font_weight: Some(FontWeight::MEDIUM),
                        ..HighlightStyle::default()
                    },
                ));
                opened = None;
            }
            _ => {}
        }
    }
    StyledText::new(step.to_owned()).with_highlights(bold)
}

fn text_button(id: &'static str, label: &'static str) -> Stateful<Div> {
    small(div())
        .id(id)
        .flex_none()
        .text_color(color(theme::FAINT))
        .cursor_pointer()
        .hover(|style| style.text_color(color(theme::MUTED)))
        .child(label)
}

pub(crate) fn plan_view(plan: &SwitchPlan, generated: Option<String>) -> Div {
    let active = plan.steps.iter().filter(|step| !step.skipped).count();
    let strategy = if plan.strategy == SwitchStrategy::Remembered {
        pill(PillTone::Ok, None, "Remembered session")
    } else {
        pill(PillTone::Warn, None, "Needs credentials")
    };
    let runs = if plan.executable {
        pill(
            PillTone::Info,
            None,
            format!("{active} of {} steps would run", plan.steps.len()),
        )
    } else {
        pill(PillTone::Fail, None, "Cannot run yet")
    };
    let state = format!(
        "{} is {}{}.",
        plan.launcher.display_name(),
        if plan.running {
            "running"
        } else {
            "not running"
        },
        match &plan.most_recent_login {
            Some(login) if !login.is_empty() => format!(", currently set to sign in as {login}"),
            _ => String::new(),
        }
    );
    let list = |paint: Paint, glyph: &'static str, hex: u32, lines: &[String]| {
        (!lines.is_empty()).then(|| {
            small(framed(paint))
                .py(px(8.))
                .flex()
                .flex_col()
                .gap(px(6.))
                .children(
                    lines
                        .iter()
                        .map(|line| marked(glyph, 14., hex, line.clone())),
                )
        })
    };
    let steps = plan.steps.iter().enumerate().map(|(index, step)| {
        let glyph = match step.kind {
            SwitchStepKind::Process => assets::POWER,
            SwitchStepKind::Backup => assets::ARCHIVE,
            SwitchStepKind::File => assets::FILE,
            SwitchStepKind::Registry => assets::SLIDERS,
            SwitchStepKind::Launch => assets::PLAY,
            SwitchStepKind::Wait => assets::HOURGLASS,
        };
        let kind = match step.kind {
            SwitchStepKind::Process => "Process",
            SwitchStepKind::Backup => "Backup",
            SwitchStepKind::File => "File",
            SwitchStepKind::Registry => "Registry",
            SwitchStepKind::Launch => "Launch",
            SwitchStepKind::Wait => "Wait",
        };
        let head = div()
            .flex()
            .items_center()
            .gap(px(10.))
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .size(px(24.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(color(theme::BORDER))
                    .bg(color(theme::SURFACE))
                    .child(icon(glyph, 14., theme::MUTED)),
            )
            .child(
                small(div())
                    .flex_none()
                    .text_color(color(theme::FAINT))
                    .child(format!("{}.", index + 1)),
            )
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_size(px(14.))
                    .line_height(px(20.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(color(theme::TEXT_STRONG))
                    .child(step.title.clone()),
            )
            .child(
                tiny(div())
                    .flex_none()
                    .rounded(px(4.))
                    .bg(color(theme::SURFACE_3))
                    .px(px(6.))
                    .py(px(2.))
                    .child(kind),
            )
            .when(step.skipped, |head| {
                head.child(
                    tiny(div())
                        .flex_none()
                        .text_color(color(theme::FAINT))
                        .child("skipped"),
                )
            });
        let changes = step.changes.iter().map(|change| {
            div()
                .flex()
                .items_start()
                .gap(px(12.))
                .border_t_1()
                .border_color(alpha(theme::BORDER, 0.6))
                .py(px(4.))
                .child(
                    div()
                        .flex_none()
                        .w(px(84.))
                        .text_color(color(theme::FAINT))
                        .child(change.place.clone()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .font_family(theme::MONO)
                        .text_color(alpha(theme::DANGER, 0.8))
                        .child(if change.before.is_empty() {
                            "(none)".to_owned()
                        } else {
                            change.before.clone()
                        }),
                )
                .child(
                    div()
                        .flex_none()
                        .text_color(color(theme::FAINT))
                        .child("\u{2192}"),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .font_family(theme::MONO)
                        .text_color(color(theme::SUCCESS))
                        .child(change.after.clone()),
                )
        });
        let body = small(div())
            .mt(px(6.))
            .ml(px(34.))
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(
                div()
                    .font_family(theme::MONO)
                    .text_color(color(theme::MUTED))
                    .child(step.target.clone()),
            )
            .child(
                div()
                    .line_height(px(19.5))
                    .text_color(color(theme::TEXT))
                    .child(step.detail.clone()),
            )
            .when(!step.changes.is_empty(), |body| {
                body.child(div().mt(px(4.)).flex().flex_col().children(changes))
            });
        div()
            .rounded(px(8.))
            .border_1()
            .px(px(12.))
            .py(px(10.))
            .map(|frame| {
                if step.skipped {
                    frame
                        .border_color(alpha(theme::BORDER, 0.6))
                        .bg(alpha(theme::SURFACE_2, 0.3))
                        .opacity(0.6)
                } else {
                    frame
                        .border_color(color(theme::BORDER))
                        .bg(alpha(theme::SURFACE_2, 0.6))
                }
            })
            .child(head)
            .child(body)
    });
    div()
        .flex()
        .flex_col()
        .gap(px(16.))
        .child(
            small(div())
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(8.))
                .text_color(color(theme::MUTED))
                .child(strategy)
                .child(runs)
                .child(state),
        )
        .children(list(
            Paint::Warning,
            assets::UPDATE,
            theme::WARNING,
            &plan.warnings,
        ))
        .children(list(Paint::Quiet, assets::INFO, theme::MUTED, &plan.notes))
        .child(div().flex().flex_col().gap(px(8.)).children(steps))
        .child(
            tiny(div())
                .text_color(color(theme::FAINT))
                .child(match generated {
                    Some(at) => format!("Generated {at}. This is a preview: nothing was changed."),
                    None => "This is a preview: nothing was changed.".to_owned(),
                }),
        )
}

pub(crate) fn generated_at(plan: &SwitchPlan, cx: &App) -> Option<String> {
    let local = local_time(cx);
    date_time(&plan.generated_at, local.offset, local.clock)
}

fn picker(request: &Request, cx: &mut Context<LaunchHost>) -> Div {
    let now = SystemTime::now();
    let rows = request
        .candidates()
        .iter()
        .enumerate()
        .map(|(index, account)| account_row(request, index, account, now, cx));
    div()
        .flex()
        .flex_col()
        .rounded(px(8.))
        .border_1()
        .border_color(color(theme::BORDER))
        .overflow_hidden()
        .children(rows)
}

fn account_row(
    request: &Request,
    index: usize,
    account: &Account,
    now: SystemTime,
    cx: &mut Context<LaunchHost>,
) -> Stateful<Div> {
    let active = request.selected.as_deref() == Some(account.id.as_str());
    let used = account
        .last_used_at
        .as_deref()
        .and_then(|at| relative(at, now))
        .map_or_else(|| "Never used".to_owned(), |when| format!("Used {when}"));
    let radio = div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(16.))
        .rounded_full()
        .border_1()
        .map(|radio| {
            if active {
                radio
                    .border_color(color(theme::ACCENT))
                    .bg(color(theme::ACCENT))
                    .child(icon(assets::CHECK, 12., theme::TEXT_STRONG))
            } else {
                radio.border_color(color(theme::BORDER_STRONG))
            }
        });
    let tags = account.tags.iter().map(|tag| {
        tiny(div())
            .flex_none()
            .rounded(px(6.))
            .bg(color(theme::SURFACE_3))
            .px(px(6.))
            .py(px(2.))
            .text_color(color(theme::MUTED))
            .child(tag.clone())
    });
    let id = account.id.clone();
    div()
        .id(("account", index))
        .flex()
        .items_center()
        .gap(px(12.))
        .px(px(12.))
        .py(px(10.))
        .when(index > 0, |row| {
            row.border_t_1().border_color(alpha(theme::BORDER, 0.6))
        })
        .map(|row| {
            if active {
                row.bg(alpha(theme::ACCENT, 0.16))
            } else {
                row.hover(|style| style.bg(alpha(theme::SURFACE_2, 0.6)))
            }
        })
        .when(matches!(request.stage, Stage::Pick), |row| {
            row.cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| this.select(id.clone(), cx)))
        })
        .child(radio)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(color(theme::TEXT_STRONG))
                                .child(account.display_name.clone()),
                        )
                        .children(tags),
                )
                .child(
                    small(div())
                        .truncate()
                        .text_color(color(theme::MUTED))
                        .child(account.login.clone()),
                ),
        )
        .child(
            small(div())
                .flex_none()
                .text_color(color(theme::FAINT))
                .child(used),
        )
}

// The one place that says the running launcher will be closed and started again.
fn close_box(preflight: &LaunchPreflight, client: &str, account: &str) -> Div {
    let message = preflight
        .close_message
        .clone()
        .unwrap_or_else(|| format!("{} is running. Switching closes it.", sentence(client)));
    framed(Paint::Warning).child(marked(
        assets::POWER,
        16.,
        theme::WARNING,
        div()
            .child(div().font_weight(FontWeight::MEDIUM).child(message))
            .child(small(div()).mt(px(2.)).child(format!(
                "{} is then started again as {account}.",
                sentence(client)
            ))),
    ))
}

fn hand_over(
    attention: &LaunchAttention,
    launcher: &str,
    stopping: bool,
    copied: Option<SharedString>,
    cx: &mut Context<LaunchHost>,
) -> Div {
    let steps = attention.steps.iter().enumerate().map(|(index, step)| {
        div()
            .flex()
            .items_start()
            .gap(px(6.))
            .child(
                div()
                    .flex_none()
                    .w(px(14.))
                    .text_color(color(theme::MUTED))
                    .child(format!("{}.", index + 1)),
            )
            .child(div().flex_1().min_w_0().child(quoted_bold(step)))
    });
    let words = div()
        .child(
            div()
                .font_weight(FontWeight::MEDIUM)
                .text_color(color(theme::TEXT_STRONG))
                .child(attention.title.clone()),
        )
        .child(
            small(div())
                .mt(px(4.))
                .line_height(px(19.5))
                .text_color(color(theme::MUTED))
                .child(attention.message.clone()),
        )
        .when(!attention.steps.is_empty(), |words| {
            words.child(
                small(div())
                    .mt(px(8.))
                    .pl(px(6.))
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .line_height(px(19.5))
                    .text_color(color(theme::TEXT))
                    .children(steps),
            )
        })
        .children(attention.reason.as_ref().map(|reason| {
            tiny(div())
                .mt(px(4.))
                .font_family(theme::MONO)
                .text_color(color(theme::FAINT))
                .child(format!("{launcher}: {reason}"))
        }));
    framed(Paint::Warning)
        .py(px(12.))
        .flex()
        .flex_col()
        .gap(px(12.))
        .text_color(color(theme::TEXT))
        .child(marked(assets::KEY, 16., theme::WARNING, words))
        .child(
            div()
                .ml(px(24.))
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(8.))
                .child(
                    Button::new("handover-copy-login", Tone::Secondary, "Copy login")
                        .glyph(assets::COPY)
                        .on_click(cx.listener(|this, _, _, cx| this.copy(CopyField::Login, cx))),
                )
                .when(attention.can_copy_password, |buttons| {
                    buttons.child(
                        Button::new("handover-copy-password", Tone::Secondary, "Copy password")
                            .glyph(assets::COPY)
                            .on_click(
                                cx.listener(|this, _, _, cx| this.copy(CopyField::Password, cx)),
                            ),
                    )
                })
                .child(
                    Button::new(
                        "stop-waiting",
                        Tone::Ghost,
                        if stopping {
                            "Stopping\u{2026}"
                        } else {
                            "Stop waiting"
                        },
                    )
                    .disabled(stopping)
                    .on_click(cx.listener(|this, _, _, cx| this.stop(cx))),
                )
                .children(
                    copied.map(|copied| small(div()).text_color(color(theme::FAINT)).child(copied)),
                ),
        )
}

fn outcome(job: &LaunchJob, client: &str, plays: bool) -> Option<Div> {
    match job.state {
        JobState::Running => None,
        JobState::Done => {
            let mut launched = div()
                .flex()
                .flex_col()
                .gap(px(12.))
                .child(pill(PillTone::Ok, Some(assets::CHECK), done_pill(job)))
                .children(
                    sent_note(job)
                        .map(|note| small(div()).text_color(color(theme::MUTED)).child(note)),
                );
            if let Some(hand_off) = &job.hand_off {
                launched = launched.child(
                    div()
                        .rounded(px(8.))
                        .border_1()
                        .border_color(color(theme::BORDER))
                        .bg(color(theme::SURFACE_2))
                        .px(px(12.))
                        .py(px(10.))
                        .text_color(color(theme::TEXT_STRONG))
                        .child(marked(assets::PLAY, 16., theme::ACCENT, hand_off.clone())),
                );
            }
            Some(launched)
        }
        JobState::NeedsSignIn | JobState::Cancelled => Some(
            framed(Paint::Warning)
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(marked(
                    assets::KEY,
                    16.,
                    theme::WARNING,
                    job.error.clone().unwrap_or_default(),
                ))
                .children(
                    stopped_note(job, client, if plays { "Play" } else { "Switch" })
                        .map(|note| small(div()).ml(px(24.)).child(note)),
                ),
        ),
        JobState::Failed => Some(
            framed(Paint::Danger)
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(marked(
                    assets::FAILED,
                    16.,
                    theme::DANGER,
                    job.error
                        .clone()
                        .unwrap_or_else(|| "The switch failed.".to_owned()),
                ))
                .child(small(div()).ml(px(24.)).child(failed_note(job, client))),
        ),
    }
}

fn phases(job: &LaunchJob, client: &str) -> Div {
    let names = PhaseNames {
        launcher: client,
        account: &job.account.display_name,
        game: job.game.as_ref().map(|game| game.name.as_str()),
    };
    let running = job.state == JobState::Running;
    let rows = group_steps(&job.steps, running)
        .into_iter()
        .enumerate()
        .map(|(index, phase)| {
            let yours = waits_for_you(job, phase.id);
            let status = if yours {
                JobStepStatus::Pending
            } else {
                phase.status
            };
            let label = format!(
                "{}{}",
                phase_label(phase.id, &names),
                if status == JobStepStatus::Running {
                    "\u{2026}"
                } else {
                    ""
                }
            );
            // While the hand-over panel is up it says what the running phase waits for.
            let current = phase.current.filter(|_| {
                status != JobStepStatus::Done
                    && !(status == JobStepStatus::Running && job.attention.is_some())
            });
            let aside = if yours {
                Some("waiting for you")
            } else if status == JobStepStatus::Skipped {
                Some("not needed")
            } else {
                None
            };
            div()
                .flex()
                .items_start()
                .gap(px(10.))
                .text_color(color(if waits(status) {
                    theme::MUTED
                } else {
                    theme::TEXT
                }))
                .child(
                    div()
                        .mt(px(2.))
                        .flex_none()
                        .child(step_mark(status, ("phase-mark", index))),
                )
                .child(
                    div()
                        .min_w_0()
                        .child(
                            div()
                                .flex()
                                .items_baseline()
                                .gap(px(8.))
                                .child(label)
                                .children(aside.map(|aside| {
                                    tiny(div()).text_color(color(theme::FAINT)).child(aside)
                                })),
                        )
                        .children(current.map(|step| {
                            small(div())
                                .text_color(color(theme::MUTED))
                                .child(step.title.clone())
                        })),
                )
        });
    div().flex().flex_col().gap(px(8.)).children(rows)
}

// Every step of the adapter with its own line of detail, behind "Show details".
fn steps(job: &LaunchJob) -> Div {
    let rows = job.steps.iter().enumerate().map(|(index, step)| {
        let title = format!(
            "{}{}",
            step.title,
            if step.status == JobStepStatus::Running {
                "\u{2026}"
            } else {
                ""
            }
        );
        let skipped = step.status == JobStepStatus::Skipped;
        div()
            .flex()
            .items_start()
            .gap(px(10.))
            .text_color(color(if waits(step.status) {
                theme::MUTED
            } else {
                theme::TEXT
            }))
            .child(
                div()
                    .mt(px(1.))
                    .flex_none()
                    .child(step_mark(step.status, ("step-mark", index))),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(div().flex().items_baseline().gap(px(8.)).child(title).when(
                        skipped,
                        |line| {
                            line.child(tiny(div()).text_color(color(theme::FAINT)).child("skipped"))
                        },
                    ))
                    .when(!step.detail.is_empty() && !skipped, |words| {
                        words.child(
                            tiny(div())
                                .truncate()
                                .font_family(theme::MONO)
                                .text_color(color(theme::FAINT))
                                .child(step.detail.clone()),
                        )
                    }),
            )
    });
    small(div())
        .flex()
        .flex_col()
        .gap(px(6.))
        .rounded(px(8.))
        .border_1()
        .border_color(color(theme::BORDER))
        .bg(alpha(theme::SURFACE_2, 0.4))
        .px(px(12.))
        .py(px(8.))
        .children(rows)
}

impl LaunchHost {
    fn takes(&self, click: &ClickEvent) -> bool {
        self.request
            .as_ref()
            .is_some_and(|request| request.appeared.takes(click))
    }

    fn toggle_plan(&mut self, cx: &mut Context<Self>) {
        if let Some(request) = &mut self.request {
            request.plan_open = !request.plan_open;
            cx.notify();
        }
    }

    fn toggle_steps(&mut self, cx: &mut Context<Self>) {
        if let Some(request) = &mut self.request {
            request.steps_open = !request.steps_open;
            cx.notify();
        }
    }

    fn plan_details(&self, request: &Request, plan: &SwitchPlan, cx: &mut Context<Self>) -> Div {
        small(div())
            .flex()
            .flex_col()
            .gap(px(8.))
            .text_color(color(theme::MUTED))
            .child(
                div()
                    .id("plan-details")
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_plan(cx)))
                    .child(div().text_size(px(8.)).child(if request.plan_open {
                        "\u{25bc}"
                    } else {
                        "\u{25b6}"
                    }))
                    .child("Details"),
            )
            .when(request.plan_open, |details| {
                details.child(plan_view(plan, generated_at(plan, cx)))
            })
    }

    // What a check of the launcher found, before anything runs: the close box, the plan's warnings and notes, and the whole plan behind "Details".
    fn found(
        &self,
        request: &Request,
        preflight: &LaunchPreflight,
        cx: &mut Context<Self>,
    ) -> Vec<Div> {
        let client = request.launcher.client_name();
        let account = request
            .account()
            .map(|account| account.display_name.as_str())
            .unwrap_or_default();
        let mut found = Vec::new();
        if preflight.needs_confirmation {
            found.push(close_box(preflight, client, account));
        }
        found.extend(preflight.plan.warnings.iter().map(|warning| {
            small(framed(Paint::Plain)).child(marked(
                assets::UPDATE,
                14.,
                theme::WARNING,
                warning.clone(),
            ))
        }));
        found.extend(preflight.plan.notes.iter().map(|note| {
            small(framed(Paint::Quiet)).child(marked(assets::INFO, 14., theme::MUTED, note.clone()))
        }));
        found.push(self.plan_details(request, &preflight.plan, cx));
        found
    }

    fn render_body(
        &self,
        request: &Request,
        job: Option<&LaunchJob>,
        cx: &mut Context<Self>,
    ) -> Div {
        let game = request.game.as_ref();
        let launcher = request.launcher;
        let client = launcher.client_name();
        let body = div().flex().flex_col().gap(px(12.));
        match &request.stage {
            Stage::Unreachable => {
                let words = if self.backend.can_reach(ol_model::LauncherId::Steam) {
                    format!(
                        "{} is not available in this build yet. Its games are listed, but One Launch cannot switch its account or start them.",
                        launcher.display_name()
                    )
                } else {
                    "No launcher is available in this build yet. The games are listed, but One Launch cannot switch accounts or start them.".to_owned()
                };
                body.child(framed(Paint::Plain).child(marked(
                    assets::INFO,
                    16.,
                    theme::MUTED,
                    words,
                )))
            }
            Stage::Pick | Stage::Checking => {
                let failed = match &request.accounts {
                    Accounts::Failed(message) => Some(
                        div()
                            .text_color(color(theme::DANGER))
                            .child(format!("Could not load accounts: {message}")),
                    ),
                    Accounts::Loading | Accounts::Ready(_) => None,
                };
                let note = game
                    .filter(|game| game.needs_update && !request.candidates().is_empty())
                    .map(|game| {
                        small(framed(Paint::Warning)).child(marked(
                            assets::UPDATE,
                            14.,
                            theme::WARNING,
                            update_note(game.launcher, &game.name),
                        ))
                    });
                let empty = request.empty().then(|| {
                    let name = launcher.display_name();
                    let more = match launcher {
                        LauncherId::Steam => {
                            " Import the ones Steam remembers, or add one by hand on the Accounts page."
                        }
                        LauncherId::Battlenet => {
                            " Import the logins Battle.net has saved, or add one by hand on the Accounts page."
                        }
                        LauncherId::Epic | LauncherId::Riot => "",
                    };
                    div()
                        .text_color(color(theme::MUTED))
                        .child(format!("No {name} accounts yet.{more}"))
                });
                let list = (!request.candidates().is_empty()).then(|| picker(request, cx));
                body.children(failed)
                    .children(note)
                    .children(empty)
                    .children(list)
            }
            Stage::Confirm(preflight) => body.children(self.found(request, preflight, cx)),
            Stage::Preview(preflight) => {
                let names = PhaseNames {
                    launcher: client,
                    account: request
                        .account()
                        .map(|account| account.display_name.as_str())
                        .unwrap_or_default(),
                    game: game.map(|game| game.name.as_str()),
                };
                let would_run = plan_phases(&preflight.plan, game.is_some())
                    .into_iter()
                    .enumerate()
                    .map(|(index, phase)| {
                        div()
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .child(
                                small(div())
                                    .w(px(16.))
                                    .flex_none()
                                    .text_color(color(theme::FAINT))
                                    .child(format!("{}.", index + 1)),
                            )
                            .child(phase_label(phase, &names))
                    });
                body.child(framed(Paint::Plain).child(marked(
                    assets::FLASK,
                    16.,
                    theme::WARNING,
                    "Preview mode is on, so nothing happened. A real switch would do this:",
                )))
                .child(
                    div()
                        .pl(px(4.))
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .children(would_run),
                )
                .children(self.found(request, preflight, cx))
            }
            Stage::Blocked(preflight) => body
                .child(framed(Paint::Danger).child(marked(
                    assets::FAILED,
                    16.,
                    theme::DANGER,
                    preflight.blocked.clone().unwrap_or_default(),
                )))
                .child(self.plan_details(request, &preflight.plan, cx)),
            Stage::Error(message) => body.child(framed(Paint::Danger).child(marked(
                assets::FAILED,
                16.,
                theme::DANGER,
                message.clone(),
            ))),
            Stage::Job => {
                let Some(job) = job else {
                    return body;
                };
                let stopping = request.stopping.as_deref() == Some(job.id.as_str());
                let waiting = job
                    .attention
                    .as_ref()
                    .filter(|_| job.state == JobState::Running)
                    .map(|attention| {
                        hand_over(
                            attention,
                            launcher.display_name(),
                            stopping,
                            request.copied.clone(),
                            cx,
                        )
                    });
                body.gap(px(16.))
                    .children(outcome(job, client, game.is_some()))
                    .children(waiting)
                    .child(phases(job, client))
                    .child(
                        div().flex().child(
                            text_button(
                                "toggle-steps",
                                if request.steps_open {
                                    "Hide details"
                                } else {
                                    "Show details"
                                },
                            )
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_steps(cx))),
                        ),
                    )
                    .when(request.steps_open, |body| body.child(steps(job)))
            }
        }
    }

    fn render_footer(
        &self,
        request: &Request,
        job: Option<&LaunchJob>,
        cx: &mut Context<Self>,
    ) -> Div {
        let client = request.launcher.client_name();
        let plays = request.game.is_some();
        let (action, action_glyph) = if plays {
            ("Play", assets::PLAY)
        } else {
            ("Switch", assets::SWITCH)
        };
        let busy = self.busy();
        let running = job.is_some_and(|job| job.state == JobState::Running);
        let waiting = running && job.is_some_and(|job| job.attention.is_some());
        let status: SharedString = match (&request.note, &request.stage) {
            (Some(note), _) => note.clone(),
            (None, Stage::Job) if running && !waiting => "Working\u{2026}".into(),
            (None, Stage::Checking) => format!("Checking {client}\u{2026}").into(),
            _ => SharedString::default(),
        };
        let back = |tone: Tone, leads: bool, cx: &mut Context<Self>| {
            Button::new("back", tone, "Back")
                .leading(leads)
                .disabled(request.starting)
                .on_click(cx.listener(|this, _, _, cx| this.back_to_pick(cx)))
        };
        let close = |cx: &mut Context<Self>| {
            Button::new("close", Tone::Secondary, "Close")
                .leading(true)
                .on_click(cx.listener(|this, _, _, cx| this.close(cx)))
        };
        let mut actions: Vec<AnyElement> = Vec::new();
        match &request.stage {
            Stage::Unreachable => actions.push(close(cx)),
            Stage::Pick if request.empty() => {
                let launcher = request.launcher.display_name();
                let label = if can_import(request.launcher) {
                    format!("Import {launcher} accounts")
                } else {
                    format!("Add {launcher} account")
                };
                actions.push(
                    Button::new("to-accounts", Tone::Primary, label)
                        .glyph(assets::KEY)
                        .leading(true)
                        .on_click(cx.listener(|this, _, _, cx| this.go_to_accounts(cx))),
                );
            }
            Stage::Pick | Stage::Checking => {
                let checking = matches!(request.stage, Stage::Checking);
                let ready = request.account().is_some() && !busy;
                actions.push(
                    Button::new("cancel", Tone::Ghost, "Cancel")
                        .disabled(busy)
                        .on_click(cx.listener(|this, _, _, cx| this.dismiss(cx))),
                );
                actions.push(
                    Button::new(
                        "play",
                        Tone::Primary,
                        if checking { "Checking\u{2026}" } else { action },
                    )
                    .glyph(action_glyph)
                    .disabled(!ready)
                    .leading(true)
                    .on_click(cx.listener(
                        |this, click: &ClickEvent, _, cx| {
                            if this.takes(click) {
                                this.check(cx);
                            }
                        },
                    )),
                );
            }
            Stage::Confirm(preflight) => {
                actions.push(back(Tone::Ghost, false, cx));
                let (tone, glyph, label) = if preflight.needs_confirmation {
                    (
                        Tone::Danger,
                        assets::POWER,
                        format!("Close {client} and continue"),
                    )
                } else {
                    (Tone::Primary, action_glyph, "Continue".to_owned())
                };
                let confirm_close = preflight.needs_confirmation;
                actions.push(
                    Button::new(
                        "continue",
                        tone,
                        if request.starting {
                            "Starting\u{2026}".to_owned()
                        } else {
                            label
                        },
                    )
                    .glyph(glyph)
                    .disabled(request.starting)
                    .on_click(cx.listener(
                        move |this, click: &ClickEvent, _, cx| {
                            if this.takes(click) {
                                this.start(confirm_close, cx);
                            }
                        },
                    )),
                );
            }
            Stage::Preview(_) => {
                actions.push(back(Tone::Ghost, false, cx));
                actions.push(
                    Button::new(
                        "switch-now",
                        Tone::Primary,
                        if plays {
                            "Turn preview off and play now"
                        } else {
                            "Turn preview off and switch now"
                        },
                    )
                    .glyph(action_glyph)
                    .disabled(request.starting)
                    .on_click(cx.listener(
                        |this, click: &ClickEvent, _, cx| {
                            if this.takes(click) {
                                this.switch_now(cx);
                            }
                        },
                    )),
                );
            }
            Stage::Blocked(_) => actions.push(back(Tone::Secondary, true, cx)),
            Stage::Error(_) => actions.push(back(Tone::Secondary, false, cx)),
            Stage::Job if running => {
                let stopping = request.stopping.is_some();
                // The hand-over panel has its own "Stop waiting".
                if !waiting {
                    actions.push(
                        Button::new(
                            "stop",
                            Tone::Ghost,
                            if stopping { "Stopping\u{2026}" } else { "Stop" },
                        )
                        .glyph(assets::STOP)
                        .tip(format!(
                            "Stop the switch. Until {client} is started again, anything already changed is put back; after that, {client} finishes signing in on its own."
                        ))
                        .disabled(stopping)
                        .on_click(cx.listener(|this, _, _, cx| this.stop(cx))),
                    );
                }
                actions.push(
                    Button::new("hide", Tone::Ghost, "Hide")
                        .glyph(assets::HIDE)
                        .tip("Keep the switch running and come back to it from the banner")
                        .on_click(cx.listener(|this, _, _, cx| this.close(cx))),
                );
            }
            Stage::Job => {
                if job.is_some_and(|job| job.state != JobState::Done) {
                    actions.push(
                        Button::new("try-again", Tone::Secondary, "Try again")
                            .glyph(assets::RETRY)
                            .on_click(cx.listener(|this, _, _, cx| this.try_again(cx))),
                    );
                }
                actions.push(
                    Button::new("done", Tone::Primary, "Done")
                        .leading(true)
                        .on_click(cx.listener(|this, _, _, cx| this.finish(cx))),
                );
            }
        }
        div()
            .flex()
            .flex_none()
            .items_center()
            .justify_between()
            .gap(px(12.))
            .border_t_1()
            .border_color(color(theme::BORDER))
            .px(px(20.))
            .py(px(12.))
            .child(
                small(div())
                    .min_w_0()
                    .text_color(color(theme::MUTED))
                    .child(status),
            )
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(8.))
                    .children(actions),
            )
    }

    pub(super) fn render_dialog(&self, cx: &mut Context<Self>) -> Option<Stateful<Div>> {
        let request = self.request.as_ref().filter(|request| !request.silent)?;
        let job = match request.stage {
            Stage::Job => self.active.as_ref().map(|active| &active.job),
            _ => None,
        };
        let busy = self.busy();
        let game = request.game.as_ref();
        let launcher = request.launcher;
        let playing_as = job.map(|job| job.account.display_name.as_str()).or(request
            .account()
            .map(|account| account.display_name.as_str()));
        let subtitle = match (playing_as, &request.stage) {
            (Some(account), _) if game.is_some() => format!("Play as {account}"),
            (Some(account), _) => format!("Switch {} to {account}", launcher.display_name()),
            (None, Stage::Unreachable) => "Not available in this build".to_owned(),
            (None, _) if request.empty() => "Add an account first".to_owned(),
            (None, _) => "Pick the account to use".to_owned(),
        };
        let header = div()
            .flex()
            .flex_none()
            .items_start()
            .justify_between()
            .gap(px(16.))
            .border_b_1()
            .border_color(color(theme::BORDER))
            .px(px(20.))
            .py(px(16.))
            .child(
                div()
                    .min_w_0()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(16.))
                                    .line_height(px(24.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(color(theme::TEXT_STRONG))
                                    .child(game.map_or_else(
                                        || "Switch account".to_owned(),
                                        |game| game.name.clone(),
                                    )),
                            )
                            .child(
                                div()
                                    .id("launch-badge")
                                    .flex_none()
                                    .when_some(game, |badge, game| {
                                        badge.tooltip(tip(format!(
                                            "{} id: {}",
                                            launcher.display_name(),
                                            game.launcher_game_id
                                        )))
                                    })
                                    .child(launcher_badge(launcher)),
                            ),
                    )
                    .child(
                        small(div())
                            .mt(px(2.))
                            .text_color(color(theme::MUTED))
                            .child(subtitle),
                    ),
            )
            .child(
                div()
                    .id("launch-close")
                    .group("launch-close")
                    .flex_none()
                    .p(px(4.))
                    .rounded(px(4.))
                    .map(|close| {
                        if busy {
                            close.opacity(0.4)
                        } else {
                            close
                                .cursor_pointer()
                                .on_click(cx.listener(|this, _, _, cx| this.dismiss(cx)))
                        }
                    })
                    .child(icon(assets::CLEAR, 16., theme::MUTED).when(!busy, |mark| {
                        mark.group_hover("launch-close", |style| {
                            style.text_color(color(theme::TEXT))
                        })
                    })),
            );
        let body = div()
            .id("launch-body")
            .flex_initial()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.body)
            .px(px(20.))
            .py(px(16.))
            .child(self.render_body(request, job, cx));
        let card = div()
            .id("launch-dialog")
            .occlude()
            .flex()
            .flex_col()
            .w_full()
            .max_w(px(672.))
            .max_h_full()
            .overflow_hidden()
            .rounded(px(12.))
            .border_1()
            .border_color(color(theme::BORDER))
            .bg(color(theme::SURFACE))
            .shadow(vec![
                BoxShadow::new(px(0.), px(8.), hsla(0., 0., 0., 0.35)).blur_radius(px(24.)),
            ])
            .child(header)
            .child(body)
            .child(self.render_footer(request, job, cx));
        Some(
            div()
                .id("launch-backdrop")
                .occlude()
                .flex()
                .items_center()
                .justify_center()
                .p(px(24.))
                .bg(alpha(theme::BG, 0.7))
                // The second click of the double-click that opened the dialog does not close it again.
                .on_click(cx.listener(|this, click: &ClickEvent, _, cx| {
                    if click.click_count() == 1 {
                        this.dismiss(cx);
                    }
                }))
                .child(card),
        )
    }
}
