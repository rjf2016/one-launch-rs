// Asked once, at start, when the last run ended during a switch. It cannot be closed without an answer.

use std::time::SystemTime;

use gpui::prelude::*;
use gpui::{BoxShadow, Context, Div, FontWeight, Stateful, StyledText, div, hsla, px};
use ol_api::launch::{Recommend, recovery_copy};
use ol_api::library::relative;
use ol_model::SwitchRecovery;
use ol_ui_kit::theme::{self, alpha, color};

use super::{LaunchHost, RecoveryStage};
use crate::assets;
use crate::widgets::{Button, Tone, icon};

fn copy_of(found: &SwitchRecovery) -> ol_api::launch::RecoveryCopy {
    recovery_copy(
        found.phase,
        found.account_name.as_deref(),
        found.snapshot_available,
        found.launcher.display_name(),
    )
}

pub(super) fn leads_with_restore(found: &SwitchRecovery) -> bool {
    found.snapshot_available && copy_of(found).recommend == Recommend::Restore
}

impl LaunchHost {
    pub(super) fn render_recovery(&self, cx: &mut Context<Self>) -> Option<Stateful<Div>> {
        let recovery = self.recovery.as_ref()?;
        let found = &recovery.found;
        let launcher = found.launcher.display_name();
        let copy = copy_of(found);
        let restored = matches!(recovery.stage, RecoveryStage::Restored);
        let restoring = matches!(recovery.stage, RecoveryStage::Restoring);
        let failed = match &recovery.stage {
            RecoveryStage::Failed(message) => Some(message.clone()),
            _ => None,
        };

        let title = if restored {
            format!("{launcher} was put back the way it was")
        } else {
            copy.title.to_owned()
        };
        let words = if restored {
            vec![
                div()
                    .mt(px(4.))
                    .text_color(color(theme::MUTED))
                    .child(format!(
                        "Start {launcher} to check which account it signs in as."
                    )),
            ]
        } else {
            let started = relative(&found.started_at, SystemTime::now()).unwrap_or_default();
            let before = format!("Started {started}. Snapshot ");
            let after = if found.snapshot_available {
                format!(". {launcher} has to be closed for a restore.")
            } else {
                ".".to_owned()
            };
            let id = before.len()..before.len() + found.backup_id.len();
            let meta = StyledText::new(format!("{before}{}{after}", found.backup_id))
                .with_font_family_overrides([(id, theme::MONO.into())]);
            vec![
                div()
                    .mt(px(4.))
                    .text_color(color(theme::MUTED))
                    .child(copy.body),
                div()
                    .mt(px(8.))
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(color(theme::FAINT))
                    .child(meta),
            ]
        };
        let mark = if restored {
            icon(assets::CHECK, 20., theme::SUCCESS)
        } else {
            icon(assets::UPDATE, 20., theme::WARNING)
        };

        let leads_with_restore = copy.recommend == Recommend::Restore;
        let tone = |leads: bool| {
            if leads { Tone::Primary } else { Tone::Ghost }
        };
        let actions = if restored {
            vec![
                Button::new("recovery-done", Tone::Primary, "Done")
                    .leading(true)
                    .on_click(cx.listener(|this, _, _, cx| this.recovery_done(cx))),
            ]
        } else if found.snapshot_available {
            vec![
                Button::new("recovery-keep", tone(!leads_with_restore), "Keep as is")
                    .leading(!leads_with_restore && failed.is_none())
                    .disabled(restoring)
                    .on_click(cx.listener(|this, _, _, cx| this.keep(cx))),
                Button::new(
                    "recovery-restore",
                    tone(leads_with_restore),
                    if restoring {
                        "Restoring\u{2026}"
                    } else if failed.is_some() {
                        "Try the restore again"
                    } else {
                        "Restore snapshot"
                    },
                )
                .leading(leads_with_restore && failed.is_none())
                .disabled(restoring)
                .on_click(cx.listener(|this, _, _, cx| this.restore(cx))),
            ]
        } else {
            vec![
                Button::new("recovery-ok", Tone::Primary, "OK")
                    .leading(true)
                    .on_click(cx.listener(|this, _, _, cx| this.keep(cx))),
            ]
        };

        let card = div()
            .id("recovery-dialog")
            .occlude()
            .w_full()
            .max_w(px(512.))
            .rounded(px(12.))
            .border_1()
            .border_color(color(theme::BORDER))
            .bg(color(theme::SURFACE))
            .shadow(vec![
                BoxShadow::new(px(0.), px(8.), hsla(0., 0., 0., 0.35)).blur_radius(px(24.)),
            ])
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(12.))
                    .px(px(20.))
                    .py(px(16.))
                    .child(mark.mt(px(2.)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(color(theme::TEXT_STRONG))
                                    .child(title),
                            )
                            .children(words)
                            .children(failed.map(|message| {
                                div()
                                    .mt(px(8.))
                                    .text_color(color(theme::DANGER))
                                    .child(message)
                            })),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap(px(8.))
                    .border_t_1()
                    .border_color(color(theme::BORDER))
                    .px(px(20.))
                    .py(px(12.))
                    .children(actions),
            );
        Some(
            div()
                .id("recovery-backdrop")
                .occlude()
                .flex()
                .items_center()
                .justify_center()
                .p(px(24.))
                .bg(alpha(theme::BG, 0.7))
                .child(card),
        )
    }
}
