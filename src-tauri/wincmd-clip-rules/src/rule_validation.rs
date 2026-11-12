// SPDX-License-Identifier: AGPL-3.0-or-later
use crate::{Action, Channel, Rule};
fn clipboard_mutation(action: &Action) -> bool {
    matches!(
        action,
        Action::MaskMatches | Action::ClearClipboard | Action::QuarantineClipboard
    )
}
pub(crate) fn valid_options(rule: &Rule) -> bool {
    let options = &rule.options;
    if options.threshold > 100
        || options.channels.is_empty()
        || options.channels.len() > 3
        || options.keywords.len() > 64
        || options.context.len() > 64
        || options.allow_list.len() > 256
        || options.channel_actions.len() > 3
        || rule.actions.len() > 7
        || options
            .keywords
            .iter()
            .chain(&options.context)
            .chain(&options.allow_list)
            .any(|s| s.is_empty() || s.len() > 2048)
        || options
            .channels
            .iter()
            .enumerate()
            .any(|(i, c)| options.channels[..i].contains(c))
        || rule
            .actions
            .iter()
            .enumerate()
            .any(|(i, a)| rule.actions[..i].contains(a))
    {
        return false;
    }
    if rule.actions.iter().any(clipboard_mutation)
        && options.channels.iter().any(|c| *c != Channel::Clipboard)
    {
        return false;
    }
    !options
        .channel_actions
        .iter()
        .enumerate()
        .any(|(i, entry)| {
            !options.channels.contains(&entry.channel)
                || options.channel_actions[..i]
                    .iter()
                    .any(|p| p.channel == entry.channel)
                || entry.actions.len() > 7
                || entry
                    .actions
                    .iter()
                    .enumerate()
                    .any(|(i, a)| entry.actions[..i].contains(a))
                || (entry.channel != Channel::Clipboard
                    && entry.actions.iter().any(clipboard_mutation))
        })
}
