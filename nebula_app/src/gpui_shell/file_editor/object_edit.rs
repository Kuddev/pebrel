//! 对象输入是独立草稿，只有完成时才进入文档历史。异步结果由当前输入实体拥有。

use super::object_source::{ObjectDraft, ObjectSource, ObjectValue};
use super::*;
use gpui::ImageCache;
use std::ops::Range;

pub(super) struct ObjectEdit {
    pub block: usize,
    pub part: usize,
    pub input: Entity<InputState>,
    pub alt: Option<Entity<InputState>>,
    pub syntax: ObjectSource,
    pub preview: String,
    pub image: Option<gpui::Resource>,
    pub error: Option<Message>,
    pub warning: Option<Message>,
    pub focus_lines: RefCell<[super::super::motion::FocusUnderline; 2]>,
    pub pending: bool,
    pub changed: bool,
    range: Range<usize>,
    original: String,
    replacement: String,
    initial_text: String,
    initial_alt: String,
    text: String,
    alt_text: String,
    sequence: u64,
    revision: u64,
    pub(super) task: Option<Task<()>>,
    picker: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl TextFileView {
    pub(super) fn object_editable(&self) -> bool {
        self.markdown
            && self.render_active
            && self.preview
            && !self.loading
            && !self.outline.limited
            && self.document.as_ref().is_some_and(|document| !document.read_only)
    }

    pub(super) fn begin_object_edit(
        &mut self,
        block: usize,
        part: usize,
        click: Option<&gpui::ClickEvent>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.object_editable() {
            return;
        }
        if let Some(gpui::ClickEvent::Mouse(click)) = click {
            let delta = click.up.position - click.down.position;
            if f32::from(delta.x).abs() + f32::from(delta.y).abs() > 4.0 {
                return;
            }
        }
        if let Some(edit) = &mut self.object_edit {
            if edit.block == block && edit.part == part {
                edit.input.update(cx, |input, cx| input.focus(window, cx));
                return;
            }
            if edit.changed {
                edit.warning = Some(Message::EditorObjectFinishFirst);
                edit.input.update(cx, |input, cx| input.focus(window, cx));
                cx.notify();
                return;
            }
        }
        let Some(structure) = self.outline.structures.get(block).and_then(Option::as_ref) else {
            return;
        };
        let Some(spec) = structure.parts.get(part) else { return };
        let block_start = self.outline.source_ranges[block].start;
        let range = block_start + spec.range.start..block_start + spec.range.end;
        if range.len() > 32 * 1024 {
            return;
        }
        let Some(original) = self.source_slice(range.clone(), cx) else { return };
        let Some(value) = ObjectValue::parse(&spec.kind, &original, &self.outline, block) else {
            return;
        };
        let preview = if matches!(value.syntax, ObjectSource::Math { display: true, .. }) {
            format!("$$\n{original}\n$$")
        } else {
            self.outline.part_preview(block, spec, &original)
        };
        self.cancel_object_edit(cx);
        self.finish_live_edit(cx);
        let input = cx.new(|cx| {
            let mut input = InputState::new(window, cx);
            if matches!(value.syntax, ObjectSource::Math { .. }) {
                input = input.auto_grow(3, 10).soft_wrap(true);
            }
            input.set_value(value.text.clone(), window, cx);
            input
        });
        let alt = value.alt.as_ref().map(|alt| {
            cx.new(|cx| {
                let mut input = InputState::new(window, cx);
                input.set_value(alt.clone(), window, cx);
                input
            })
        });
        let subscriptions = std::iter::once(&input)
            .chain(alt.iter())
            .map(|field| {
                cx.subscribe_in(field, window, |view, _, event, _, cx| match event {
                    InputEvent::Change => view.refresh_object_draft(cx),
                    InputEvent::Focus | InputEvent::Blur => cx.notify(),
                    _ => {},
                })
            })
            .collect();
        self.object_edit = Some(ObjectEdit {
            block,
            part,
            input: input.clone(),
            alt,
            syntax: value.syntax,
            preview,
            image: None,
            error: None,
            warning: None,
            focus_lines: Default::default(),
            pending: true,
            changed: false,
            range,
            original: original.clone(),
            replacement: original,
            initial_text: value.text.clone(),
            initial_alt: value.alt.clone().unwrap_or_default(),
            text: value.text,
            alt_text: value.alt.unwrap_or_default(),
            sequence: 0,
            revision: self.content_revision,
            task: None,
            picker: None,
            _subscriptions: subscriptions,
        });
        self.preview_task = None;
        self.stop_preview_selection_scroll();
        self.all_selected = false;
        self.refresh_object_draft(cx);
        self.invalidate_live_block(block);
        input.update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    pub(super) fn refresh_object_draft(&mut self, cx: &mut Context<Self>) {
        let Some(edit) = &mut self.object_edit else { return };
        let text = edit.input.read(cx).value().to_string();
        let alt =
            edit.alt.as_ref().map(|input| input.read(cx).value().to_string()).unwrap_or_default();
        if text == edit.text && alt == edit.alt_text && edit.task.is_some() {
            return;
        }
        edit.text = text.clone();
        edit.alt_text = alt.clone();
        let changed = text != edit.initial_text || alt != edit.initial_alt;
        let dirty_changed = changed != edit.changed;
        edit.changed = changed;
        edit.pending = true;
        edit.error = None;
        edit.warning = None;
        edit.sequence = edit.sequence.wrapping_add(1);
        let sequence = edit.sequence;
        let input = edit.input.clone();
        let syntax = edit.syntax.clone();
        let base = self.path.parent().map(PathBuf::from);
        let executor = cx.background_executor().clone();
        edit.task = Some(cx.spawn(async move |this, cx| {
            executor.timer(Duration::from_millis(100)).await;
            let result =
                executor.spawn(async move { syntax.draft(&text, &alt, base.as_deref()) }).await;
            let _ = this.update(cx, |view, cx| {
                let Some(edit) = &mut view.object_edit else { return };
                if edit.input != input || edit.sequence != sequence {
                    return;
                }
                edit.pending = false;
                match result {
                    Ok(ObjectDraft { replacement, preview, image }) => {
                        edit.replacement =
                            if edit.changed { replacement } else { edit.original.clone() };
                        edit.preview = preview;
                        edit.image = image;
                        edit.error = None;
                    },
                    Err(message) => edit.error = Some(message),
                }
                let block = edit.block;
                view.invalidate_live_block(block);
                cx.notify();
            });
        }));
        if dirty_changed {
            cx.emit(TextFileEvent::Changed);
        }
        cx.notify();
    }

    pub(super) fn object_composing(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.object_edit.as_ref().is_some_and(|edit| {
            std::iter::once(&edit.input).chain(edit.alt.iter()).any(|field| {
                field.update(cx, |input, cx| input.marked_text_range(window, cx).is_some())
            })
        })
    }

    pub(super) fn settle_object_composition(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.object_composing(window, cx) {
            return false;
        }
        if let Some(edit) = &self.object_edit {
            for field in std::iter::once(&edit.input).chain(edit.alt.iter()) {
                field.update(cx, |input, cx| input.unmark_text(window, cx));
            }
        }
        self.refresh_object_draft(cx);
        true
    }

    pub(super) fn apply_object_edit(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.object_edit.is_none() {
            return true;
        }
        if self.object_composing(window, cx) {
            return false;
        }
        let Some(edit) = &self.object_edit else { return true };
        if edit.input.read(cx).value().as_ref() != edit.text
            || edit
                .alt
                .as_ref()
                .is_some_and(|input| input.read(cx).value().as_ref() != edit.alt_text)
        {
            self.refresh_object_draft(cx);
            return false;
        }
        if !edit.changed {
            self.cancel_object_edit(cx);
            self.focus.focus(window, cx);
            return true;
        }
        if edit.pending || edit.error.is_some() {
            return false;
        }
        if !self.object_editable()
            || edit.revision != self.content_revision
            || self.source_slice(edit.range.clone(), cx).as_deref() != Some(&edit.original)
        {
            self.object_edit.as_mut().unwrap().error = Some(Message::EditorObjectConflict);
            cx.notify();
            return false;
        }
        if let Some(resource) = &edit.image {
            if !self
                .preview_images
                .update(cx, |cache, cx| cache.load(resource, window, cx))
                .is_some_and(|result| result.is_ok())
            {
                return false;
            }
        }
        let edit = self.object_edit.take().unwrap();
        if matches!(self.notice, Some((Message::EditorObjectFinishFirst, _))) {
            self.notice = None;
        }
        if edit.original != edit.replacement {
            self.commit_source_edit(edit.range, &edit.replacement, None, window, cx);
            // 阅读器只撤销已完成的对象事务；普通源码历史继续由源码模式拥有。
            self.history.mark_object_edit();
        } else {
            self.invalidate_live_block(edit.block);
        }
        self.focus.focus(window, cx);
        cx.emit(TextFileEvent::Changed);
        cx.notify();
        true
    }

    pub(super) fn cancel_object_edit(&mut self, cx: &mut Context<Self>) {
        if let Some(edit) = self.object_edit.take() {
            if matches!(self.notice, Some((Message::EditorObjectFinishFirst, _))) {
                self.notice = None;
            }
            self.invalidate_live_block(edit.block);
            cx.emit(TextFileEvent::Changed);
            cx.notify();
        }
    }

    pub(super) fn choose_object_image(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(edit) = &self.object_edit else { return };
        if edit.alt.is_none() {
            return;
        }
        let input = edit.input.clone();
        let sequence = edit.sequence;
        let picked = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(
                super::super::config::ui_language(cx).text(Message::EditorObjectChooseImage).into(),
            ),
        });
        let task = cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else { return };
            let Some(path) = paths.first() else { return };
            let _ = this.update_in(cx, |view, window, cx| {
                if view
                    .object_edit
                    .as_ref()
                    .is_none_or(|edit| edit.input != input || edit.sequence != sequence)
                {
                    return;
                }
                // 选择器返回时仍需核对输入身份；关闭后重开的对象不能接收旧文件。
                let address = path.to_string_lossy().replace('\\', "/");
                input.update(cx, |input, cx| input.set_value(address, window, cx));
                view.refresh_object_draft(cx);
            });
        });
        self.object_edit.as_mut().unwrap().picker = Some(task);
    }
}
