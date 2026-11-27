//! Page initiated dialogs and pickers: alert/confirm/prompt, <select> popups and the context
//! menu. Servo hands them over as one-shot objects, we park them here, the sidebar UI renders
//! the front one as an overlay and answers with a `dialog_result` bridge command.
//!
//! Anything we can't show (file and colour pickers for now) is dropped, which Servo treats
//! as the user cancelling.

use std::collections::VecDeque;

use serde::Serialize;
use servo::{
    ContextMenu, ContextMenuItem, EmbedderControl, EmbedderControlId, SelectElement,
    SelectElementOptionOrOptgroup, SimpleDialog,
};

use crate::layout::CssRect;
use crate::tabs::TabId;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DialogKind {
    Alert,
    Confirm,
    Prompt,
    Select,
    Menu,
}

#[derive(Clone, Debug, Serialize)]
pub struct DialogOption {
    pub index: usize,
    pub label: String,
    pub disabled: bool,
    pub group: Option<String>,
    pub selected: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct DialogView {
    pub id: u64,
    pub kind: DialogKind,
    /// Who is asking, shown above page dialogs so they can't pass for browser UI.
    pub origin: String,
    pub message: String,
    pub value: String,
    pub multiple: bool,
    pub options: Vec<DialogOption>,
    /// Where the page wants the popup, in window CSS pixels.
    pub anchor: Option<CssRect>,
}

enum Control {
    Simple(SimpleDialog),
    Select(SelectElement),
    Menu(ContextMenu),
}

struct Pending {
    view: DialogView,
    tab: Option<TabId>,
    control_id: EmbedderControlId,
    control: Control,
}

#[derive(Default)]
pub struct Dialogs {
    next_id: u64,
    pending: VecDeque<Pending>,
}

impl Dialogs {
    pub fn is_open(&self) -> bool {
        !self.pending.is_empty()
    }

    pub fn current(&self) -> Option<DialogView> {
        self.pending.front().map(|p| p.view.clone())
    }

    /// Queues a control. `origin` is the page host, `offset` the pane position in window
    /// pixels so popups land next to the element that opened them. Returns false when the
    /// control was dropped instead.
    pub fn show(
        &mut self,
        tab: Option<TabId>,
        control: EmbedderControl,
        origin: String,
        offset: (i32, i32),
        scale: f32,
    ) -> bool {
        self.next_id += 1;
        let id = self.next_id;
        let control_id = control.id();
        let anchor = |r: servo::DeviceIntRect| {
            crate::layout::Rect::new(
                r.min.x + offset.0,
                r.min.y + offset.1,
                r.width(),
                r.height(),
            )
            .to_css(scale)
        };
        let blank = |kind| DialogView {
            id,
            kind,
            origin: origin.clone(),
            message: String::new(),
            value: String::new(),
            multiple: false,
            options: Vec::new(),
            anchor: None,
        };

        let (view, control) = match control {
            EmbedderControl::SimpleDialog(dialog) => {
                let kind = match &dialog {
                    SimpleDialog::Alert(_) => DialogKind::Alert,
                    SimpleDialog::Confirm(_) => DialogKind::Confirm,
                    SimpleDialog::Prompt(_) => DialogKind::Prompt,
                };
                let mut view = blank(kind);
                view.message = dialog.message().to_owned();
                if let SimpleDialog::Prompt(prompt) = &dialog {
                    view.value = prompt.current_value().to_owned();
                }
                (view, Control::Simple(dialog))
            }
            EmbedderControl::SelectElement(select) => {
                let mut view = blank(DialogKind::Select);
                let selected = select.selected_options();
                view.multiple = select.allow_select_multiple();
                view.anchor = Some(anchor(select.position()));
                let mut push = |o: &servo::SelectElementOption, group: Option<&String>| {
                    view.options.push(DialogOption {
                        index: o.id,
                        label: o.label.clone(),
                        disabled: o.is_disabled,
                        group: group.cloned(),
                        selected: selected.contains(&o.id),
                    })
                };
                for entry in select.options() {
                    match entry {
                        SelectElementOptionOrOptgroup::Option(o) => push(o, None),
                        SelectElementOptionOrOptgroup::Optgroup { label, options } => {
                            for o in options {
                                push(o, Some(label));
                            }
                        }
                    }
                }
                (view, Control::Select(select))
            }
            EmbedderControl::ContextMenu(menu) => {
                let mut view = blank(DialogKind::Menu);
                view.anchor = Some(anchor(menu.position()));
                for (index, item) in menu.items().iter().enumerate() {
                    let (label, disabled) = match item {
                        ContextMenuItem::Item { label, enabled, .. } => (label.clone(), !enabled),
                        // hmm, an empty label is how the UI knows to draw a separator line
                        ContextMenuItem::Separator => (String::new(), true),
                    };
                    view.options.push(DialogOption {
                        index,
                        label,
                        disabled,
                        group: None,
                        selected: false,
                    });
                }
                (view, Control::Menu(menu))
            }
            // dropping these cancels them on the Servo side
            _ => return false,
        };

        self.pending.push_back(Pending {
            view,
            tab,
            control_id,
            control,
        });
        true
    }

    /// Applies the user's answer to dialog `id`. `value` is a string for prompts, an array of
    /// option indices for selects and a single item index for menus.
    pub fn resolve(&mut self, id: u64, ok: bool, value: Option<serde_json::Value>) {
        let Some(at) = self.pending.iter().position(|p| p.view.id == id) else {
            return;
        };
        let Some(pending) = self.pending.remove(at) else {
            return;
        };
        match pending.control {
            Control::Simple(SimpleDialog::Alert(alert)) => alert.confirm(),
            Control::Simple(SimpleDialog::Confirm(confirm)) => {
                if ok {
                    confirm.confirm()
                } else {
                    confirm.dismiss()
                }
            }
            Control::Simple(SimpleDialog::Prompt(mut prompt)) => {
                if ok {
                    if let Some(text) = value.as_ref().and_then(|v| v.as_str()) {
                        prompt.set_current_value(text);
                    }
                    prompt.confirm()
                } else {
                    prompt.dismiss()
                }
            }
            Control::Select(mut select) => {
                if ok {
                    let picked: Vec<usize> = value
                        .as_ref()
                        .and_then(|v| v.as_array())
                        .map(|a| {
                            a.iter()
                                .filter_map(|i| i.as_u64())
                                .map(|i| i as usize)
                                .collect()
                        })
                        .unwrap_or_default();
                    select.select(picked);
                    select.submit();
                }
            }
            Control::Menu(menu) => {
                let action = value
                    .as_ref()
                    .and_then(|v| v.as_u64())
                    .and_then(|i| menu.items().get(i as usize))
                    .and_then(|item| match item {
                        ContextMenuItem::Item {
                            action, enabled, ..
                        } if *enabled => Some(*action),
                        _ => None,
                    });
                match action {
                    Some(action) if ok => menu.select(action),
                    _ => menu.dismiss(),
                }
            }
        }
    }

    /// Servo closed a control on its own (page navigated away, element removed).
    pub fn hide(&mut self, control_id: EmbedderControlId) {
        self.pending.retain(|p| p.control_id != control_id);
    }

    pub fn drop_for_tab(&mut self, tab: TabId) {
        self.pending.retain(|p| p.tab != Some(tab));
    }
}
