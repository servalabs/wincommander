// SPDX-License-Identifier: AGPL-3.0-or-later
mod common;
use wincmd_clip_rules::*;
#[test]
fn clipboard_mutations_are_rejected_on_file_channels() {
    for action in [
        Action::MaskMatches,
        Action::ClearClipboard,
        Action::QuarantineClipboard,
    ] {
        let mut rule = common::phrase_rule(1, 100, "sensitive");
        rule.actions = vec![action];
        assert!(compile(&[rule.clone()], &RuleSetLimits::default()).is_ok());
        rule.options.channels = vec![Channel::Removable];
        assert!(compile(&[rule.clone()], &RuleSetLimits::default()).is_err());
        rule.options.channels = vec![Channel::Clipboard, Channel::Removable];
        assert!(compile(&[rule.clone()], &RuleSetLimits::default()).is_err());
        rule.actions = vec![Action::NotifyUser];
        rule.options.channel_actions = vec![ChannelActions {
            channel: Channel::Removable,
            actions: vec![action],
        }];
        assert!(compile(&[rule.clone()], &RuleSetLimits::default()).is_err());
        rule.options.channel_actions[0].channel = Channel::Clipboard;
        let compiled = compile(&[rule], &RuleSetLimits::default()).unwrap();
        assert_eq!(
            compiled.scan("sensitive", Channel::Clipboard).findings[0].actions,
            vec![action]
        );
        assert_eq!(
            compiled.scan("sensitive", Channel::Removable).findings[0].actions,
            vec![Action::NotifyUser]
        );
    }
}
