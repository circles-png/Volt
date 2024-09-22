use itertools::Itertools;
use std::{
    cmp::Ordering,
    collections::HashSet,
    fs::{read_dir, File},
    io,
    path::PathBuf,
    thread::JoinHandle,
};
use strum::Display;

// FIXME: Temporary rodio playback, might need to use cpal or make rodio proper
use egui::{
    include_image, pos2, vec2, Align2, Context, FontFamily, FontId, Image, LayerId, PointerButton,
    Pos2, Rect, Stroke, Ui,
};
use open::that_detached;
use rodio::{Decoder, OutputStream, Sink};

use std::io::BufReader;

use unicode_truncate::UnicodeTruncateStr;

use crate::visual::ThemeColors;

fn hovered(ctx: &Context, rect: &Rect) -> bool {
    ctx.rect_contains_pointer(
        ctx.layer_id_at(ctx.pointer_hover_pos().unwrap_or_default())
            .unwrap_or_else(LayerId::background),
        *rect,
    )
}

#[derive(Display, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Files,
    Devices,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub path: PathBuf,
    pub kind: EntryKind,
}

impl Ord for Entry {
    fn cmp(&self, other: &Self) -> Ordering {
        Ordering::then(
            Ord::cmp(&self.kind, &other.kind),
            Ord::cmp(
                self.path.file_name().unwrap_or_default(),
                other.path.file_name().unwrap_or_default(),
            ),
        )
    }
}

impl PartialOrd for Entry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Display, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EntryKind {
    UpOneLevel,
    Directory,
    Audio,
    File,
}

pub struct Preview {
    pub preview_thread: Option<JoinHandle<()>>,
}

impl Preview {
    pub fn play_file(&mut self, file: File) {
        // Kill the current thread if it's not sleeping
        if let Some(thread) = self.preview_thread.take() {
            if !thread.is_finished() {
                thread.thread().unpark();
            }
        }

        let file = BufReader::new(file);
        self.preview_thread = Some(std::thread::spawn(move || {
            let (_stream, stream_handle) = OutputStream::try_default().unwrap();
            let source = Decoder::new(file).unwrap();
            let sink = Sink::try_new(&stream_handle).unwrap();
            // let source = SineWave::new(440.0).take_duration(Duration::from_secs_f32(0.25)).amplify(0.20);
            sink.append(source);
            std::thread::park();
        }));
    }
}

pub struct Browser {
    pub expanded_directories: HashSet<PathBuf>,
    pub selected_category: Category,
    pub path: PathBuf,
    pub preview: Preview,
    pub offset_y: f32,
    pub dragging_audio: bool,
    pub dragging_audio_text: String,
    pub sidebar_width: f32,
    pub started_drag: bool,
}

impl Browser {
    pub fn paint_button(
        ctx: &Context,
        ui: &Ui,
        button: &Rect,
        selected: bool,
        text: &str,
        theme: &ThemeColors,
    ) {
        let color = if selected {
            theme.browser_selected_button_fg
        } else if hovered(ctx, button) {
            theme.browser_unselected_hover_button_fg
        } else {
            theme.browser_unselected_button_fg
        };
        ui.painter().text(
            button.center(),
            Align2::CENTER_CENTER,
            text,
            FontId::new(14.0, FontFamily::Name("IBMPlexMono".into())),
            color,
        );
        ui.painter().line_segment(
            [
                Pos2 {
                    x: button.left() + 8.,
                    y: button.bottom(),
                },
                Pos2 {
                    x: button.right() - 8.,
                    y: button.bottom(),
                },
            ],
            Stroke::new(0.5, color),
        );
    }

    #[allow(clippy::too_many_lines, clippy::cognitive_complexity)]
    pub fn paint(&mut self, ctx: &Context, ui: &mut Ui, viewport: &Rect, theme: &ThemeColors) {
        ui.painter().rect_filled(
            Rect {
                min: Pos2 { x: 0., y: 50. },
                max: Pos2 {
                    x: self.sidebar_width,
                    y: viewport.height(),
                },
            },
            0.0,
            theme.browser,
        );
        ui.painter().line_segment(
            [
                Pos2 {
                    x: self.sidebar_width,
                    y: 50.,
                },
                Pos2 {
                    x: self.sidebar_width,
                    y: viewport.height(),
                },
            ],
            Stroke::new(0.5, theme.browser_outline),
        );
        let (was_pressed, was_double_clicked, press_position) = ctx
            .input(|input_state| {
                Some((
                    input_state.pointer.button_released(PointerButton::Primary),
                    input_state
                        .pointer
                        .button_double_clicked(PointerButton::Primary),
                    Some(input_state.pointer.latest_pos()?),
                ))
            })
            .unwrap_or_default();
        for (category, rect) in [
            (
                Category::Files,
                Rect::from_min_size(pos2(0., 55.), vec2(self.sidebar_width / 2., 30.)),
            ),
            (
                Category::Devices,
                Rect::from_min_size(
                    pos2(self.sidebar_width / 2., 55.),
                    vec2(self.sidebar_width / 2., 30.),
                ),
            ),
        ] {
            let open = self.selected_category == category;
            Self::paint_button(ctx, ui, &rect, open, category.to_string().as_str(), theme);
            if press_position
                .is_some_and(|press_position| was_pressed && rect.contains(press_position))
            {
                self.selected_category = category;
            }
        }
        let scroll = ctx.input(|i| i.smooth_scroll_delta.y);
        if let Some(pos) = ctx.input(|i| i.pointer.latest_pos()) {
            if pos.x <= self.sidebar_width && scroll != 0. {
                self.offset_y += scroll;
            }
        }
        match self.selected_category {
            Category::Files => {
                let entries = read_dir(&self.path).map_or(Vec::new(), |entries| {
                    entries
                        .map(|entry| -> io::Result<Entry> {
                            let entry = entry?;
                            Ok(Entry {
                                path: entry.path(),
                                kind: if entry.metadata()?.is_dir() {
                                    EntryKind::Directory
                                } else if [".wav", ".wave", ".mp3", ".ogg", ".flac", ".opus"]
                                    .into_iter()
                                    .any(|extension| {
                                        entry
                                            .file_name()
                                            .to_str()
                                            .unwrap_or_default()
                                            .ends_with(extension)
                                    })
                                {
                                    EntryKind::Audio
                                } else {
                                    EntryKind::File
                                },
                            })
                        })
                        .try_collect()
                        .unwrap_or_default()
                });

                // Add ".." entry if not at root
                let parent_entry = self.path.parent().map(|parent| Entry {
                    path: parent.to_path_buf(),
                    kind: EntryKind::UpOneLevel,
                });
                let max_entries = entries.len() + usize::from(parent_entry.is_some());
                let entries = parent_entry.into_iter().chain(entries).sorted_unstable();

                // Calculate the maximum offset based on the number of entries and browser height
                let browser_height = viewport.height() - 90.0; // Adjust for header height
                let bottom_margin = 8.0; // Add a slight margin at the bottom
                #[allow(clippy::cast_precision_loss)]
                let max_offset =
                    (max_entries as f32).mul_add(16.0, -browser_height) + bottom_margin;

                // Clamp the offset
                self.offset_y = self.offset_y.clamp(-max_offset.max(0.0), 0.0);

                // Handle sidebar resizing
                let resize_rect = Rect::from_min_size(
                    pos2(self.sidebar_width - 5., 50.),
                    vec2(10., viewport.height() - 50.),
                );
                if ctx.rect_contains_pointer(LayerId::background(), resize_rect) {
                    ui.output_mut(|output| output.cursor_icon = egui::CursorIcon::ResizeHorizontal);
                    if ctx.input(|input| input.pointer.primary_pressed()) {
                        self.started_drag = true;
                    }
                }
                if self.started_drag {
                    if let Some(mouse_pos) = ctx.pointer_hover_pos() {
                        self.sidebar_width = mouse_pos.x.clamp(100.0, viewport.width() / 2.0);
                    }
                    if ctx.input(|i| i.pointer.primary_released()) {
                        self.started_drag = false;
                    }
                }

                // Draw resize handle
                if let Some(mouse_pos) = ctx.pointer_hover_pos() {
                    if (mouse_pos.x - self.sidebar_width).abs() <= 20.0 {
                        ui.painter().rect_filled(
                            resize_rect,
                            0.0,
                            theme.browser_outline.gamma_multiply(0.2),
                        );
                    }
                }
                let mut current_y = 90. + self.offset_y;

                for entry in entries {
                    #[allow(clippy::cast_precision_loss)]
                    let rect =
                        &Rect::from_min_size(pos2(0., current_y), vec2(self.sidebar_width, 16.));
                    if current_y >= 90. {
                        egui::Frame::none().show(ui, |ui| {
                            ui.allocate_space(ui.available_size());
                            let mut invalid = false;
                            let name = if entry.kind == EntryKind::UpOneLevel {
                                "..".to_string()
                            } else {
                                entry.path.file_name().unwrap().to_str().map_or_else(
                                    || {
                                        invalid = true;
                                        String::from_utf8_lossy(
                                            entry.path.file_name().unwrap().as_encoded_bytes(),
                                        )
                                        .to_string()
                                    },
                                    ToString::to_string,
                                )
                            };
                            let chars_to_truncate;
                            let text_width = ui
                                .painter()
                                .layout_no_wrap(
                                    name.clone(),
                                    FontId::new(14., FontFamily::Name("IBMPlexMono".into())),
                                    theme.browser_unselected_button_fg,
                                )
                                .rect
                                .width();
                            let char_width = ui
                                .painter()
                                .layout_no_wrap(
                                    "a".to_string(),
                                    FontId::new(14., FontFamily::Name("IBMPlexMono".into())),
                                    theme.browser_unselected_button_fg,
                                )
                                .rect
                                .width();
                            if invalid {
                                ui.painter().rect_filled(
                                    Rect::from_min_size(
                                        pos2(30., current_y),
                                        vec2(text_width, 16.),
                                    ),
                                    0.0,
                                    theme.browser_invalid_name_bg,
                                );
                            }
                            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                            {
                                chars_to_truncate = (self.sidebar_width / char_width) as usize - 10;
                            }
                            ui.painter().text(
                                pos2(30., current_y),
                                Align2::LEFT_TOP,
                                if name.unicode_truncate(chars_to_truncate).1 == chars_to_truncate {
                                    name.unicode_truncate(chars_to_truncate).0.to_string() + "..."
                                } else {
                                    name
                                },
                                FontId::new(14., FontFamily::Name("IBMPlexMono".into())),
                                match (hovered(ctx, rect), invalid) {
                                    (true, true) => {
                                        theme.browser_unselected_hover_button_fg_invalid
                                    }
                                    (true, false) => theme.browser_unselected_hover_button_fg,
                                    (false, true) => theme.browser_unselected_button_fg_invalid,
                                    (false, false) => theme.browser_unselected_button_fg,
                                },
                            )
                        });

                        if let Some(image) = match entry.kind {
                            EntryKind::UpOneLevel => None,
                            EntryKind::Directory => {
                                if self.expanded_directories.contains(&entry.path) {
                                    // TODO Add a folder open icon
                                    None
                                } else {
                                    Some(include_image!("images/icons/folder.png"))
                                }
                            }
                            EntryKind::Audio => Some(include_image!("images/icons/audio.png")),
                            EntryKind::File => Some(include_image!("images/icons/file.png")),
                        }
                        .map(Image::new)
                        {
                            image.paint_at(
                                ui,
                                Rect::from_min_size(pos2(10., current_y + 2.), vec2(14., 14.)),
                            );
                        }
                    }
                    if entry.kind == EntryKind::Audio {
                        let is_dragging = ctx.input(|i| i.pointer.is_decidedly_dragging());
                        let cursor_pos = ctx.input(|i| i.pointer.hover_pos());

                        if is_dragging
                            && cursor_pos.is_some()
                            && rect.contains(cursor_pos.unwrap())
                            && !self.dragging_audio
                            && cursor_pos.unwrap().x <= self.sidebar_width - 10.
                            && !self.started_drag
                        {
                            self.dragging_audio = true;
                            self.dragging_audio_text = entry
                                .path
                                .file_name()
                                .unwrap()
                                .to_str()
                                .unwrap()
                                .to_string();
                        }

                        if let Some(cursor_pos) = cursor_pos {
                            if self.dragging_audio
                                && self.dragging_audio_text
                                    == *entry.path.file_name().unwrap().to_str().unwrap()
                            {
                                ui.painter().text(
                                    cursor_pos + vec2(5.0, 2.0),
                                    Align2::CENTER_CENTER,
                                    &self.dragging_audio_text,
                                    FontId::new(14.0, FontFamily::Name("IBMPlexMono".into())),
                                    theme.browser_selected_button_fg,
                                );
                            }
                        }

                        if !is_dragging {
                            self.dragging_audio = false;
                            self.dragging_audio_text = String::new();
                        }
                    }
                    if press_position.is_some_and(|press_position| {
                        rect.contains(press_position)
                            && !self.dragging_audio
                            // TODO make these two comparisons part of the `rect.contains` check
                            && press_position.x <= self.sidebar_width - 10.
                            && press_position.y >= 90.
                    }) {
                        let mut invert_expanded = || {
                            if !self.expanded_directories.insert(entry.path.clone()) {
                                self.expanded_directories.remove(&entry.path);
                            }
                        };
                        match (was_pressed, was_double_clicked) {
                            (_, true) => {
                                if entry.kind == EntryKind::Directory {
                                    self.path.clone_from(&entry.path);
                                    invert_expanded();
                                }
                            }
                            (true, _) => {
                                match entry.kind {
                                    EntryKind::UpOneLevel => {
                                        self.path.clone_from(&entry.path);
                                    }
                                    EntryKind::Directory => invert_expanded(),
                                    EntryKind::Audio => {
                                        // TODO: Proper preview implementation with cpal. This is temporary (or at least make it work well with a proper preview widget)
                                        // Also, don't spawn a new thread - instead, dedicate a thread for preview
                                        let file = File::open(entry.path.as_path()).unwrap();
                                        self.preview.play_file(file);
                                    }
                                    EntryKind::File => {
                                        that_detached(entry.path.clone()).unwrap();
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    current_y += 16.;
                }
            }
            Category::Devices => {
                // TODO: Show some devices here!
            }
        }
    }
}
