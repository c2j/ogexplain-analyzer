use crossterm::event::{KeyCode, KeyModifiers};

use crate::action::Action;
use crate::app::{AppMode, FocusTarget};

pub fn handle_key(
    key: crossterm::event::KeyEvent,
    _mode: AppMode,
    focus: FocusTarget,
) -> Option<Action> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);

    match (ctrl, shift, key.code) {
        // Global shortcuts (always active, even in Input mode)
        (true, _, KeyCode::Char('c')) => Some(Action::Quit),
        (true, _, KeyCode::Char('l')) => Some(Action::ClearInput),
        (false, _, KeyCode::Char('?')) => match focus {
            FocusTarget::Input => None,
            _ => Some(Action::ToggleHelp),
        },
        (false, false, KeyCode::F(1)) => Some(Action::ToggleHelp),

        (true, _, KeyCode::Char('p')) => Some(Action::ParseExplain),

        (_, true, KeyCode::BackTab) => Some(Action::PrevPanel),

        (false, false, KeyCode::Tab) => Some(Action::NextPanel),

        (false, false, KeyCode::Char('q')) => match focus {
            FocusTarget::Input => None,
            _ => Some(Action::Quit),
        },

        (false, false, KeyCode::Up) | (false, false, KeyCode::Char('k')) => match focus {
            FocusTarget::Tree => Some(Action::TreeUp),
            FocusTarget::Detail => Some(Action::DetailUp),
            _ => None,
        },

        (false, false, KeyCode::Down) | (false, false, KeyCode::Char('j')) => match focus {
            FocusTarget::Tree => Some(Action::TreeDown),
            FocusTarget::Detail => Some(Action::DetailDown),
            _ => None,
        },

        (false, false, KeyCode::Enter) => match focus {
            FocusTarget::Tree => Some(Action::TreeToggle),
            _ => None,
        },

        (false, false, KeyCode::Char('e')) => match focus {
            FocusTarget::Tree => Some(Action::TreeExpandAll),
            _ => None,
        },

        (false, false, KeyCode::Char('w')) => match focus {
            FocusTarget::Tree => Some(Action::TreeCollapseAll),
            _ => None,
        },

        (false, false, KeyCode::Char('f') | KeyCode::Char('F')) => match focus {
            FocusTarget::Tree | FocusTarget::Detail => Some(Action::ToggleFindings),
            _ => None,
        },

        // Tree shortcuts: g/G for top/bottom
        (false, false, KeyCode::Char('g')) => match focus {
            FocusTarget::Tree => Some(Action::TreeTop),
            _ => None,
        },

        (false, false, KeyCode::Char('G')) => match focus {
            FocusTarget::Tree => Some(Action::TreeBottom),
            _ => None,
        },

        // Detail shortcuts: PageUp/PageDown/Home/End
        (false, false, KeyCode::PageUp) => match focus {
            FocusTarget::Detail => Some(Action::DetailPageUp),
            _ => None,
        },

        (false, false, KeyCode::PageDown) => match focus {
            FocusTarget::Detail => Some(Action::DetailPageDown),
            _ => None,
        },

        (false, false, KeyCode::Home) => match focus {
            FocusTarget::Detail => Some(Action::DetailHome),
            _ => None,
        },

        (false, false, KeyCode::End) => match focus {
            FocusTarget::Detail => Some(Action::DetailEnd),
            _ => None,
        },

        // Raw view toggle: r when focus is Tree or Detail (not Input)
        (false, false, KeyCode::Char('r')) => match focus {
            FocusTarget::Tree | FocusTarget::Detail => Some(Action::ToggleRawView),
            _ => None,
        },

        // Complexity toggle: c when not in Input
        (false, false, KeyCode::Char('c')) => match focus {
            FocusTarget::Input => None,
            _ => Some(Action::ToggleComplexity),
        },

        // Multi-plan navigation: N/P in Browse mode
        (false, _, KeyCode::Char('n') | KeyCode::Char('N')) => match focus {
            FocusTarget::Tree | FocusTarget::Detail => Some(Action::NextPlan),
            _ => None,
        },

        (false, _, KeyCode::Char('p') | KeyCode::Char('P')) => match focus {
            FocusTarget::Tree | FocusTarget::Detail => Some(Action::PrevPlan),
            _ => None,
        },

        _ => None,
    }
}

pub fn should_passthrough(mode: AppMode, focus: FocusTarget) -> bool {
    focus == FocusTarget::Input || mode == AppMode::Input
}
